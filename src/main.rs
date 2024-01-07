use std::{
    collections::{HashMap, HashSet},
    io::{stdout, Write},
    path::{Path, PathBuf},
    process::exit,
    str::FromStr,
};

use clap::Parser;
use naga::valid::Capabilities;
use naga_oil::compose::{
    ComposableModuleDescriptor, Composer, ComposerErrorInner, NagaModuleDescriptor, ShaderDefValue,
    ShaderLanguage, ShaderType,
};

#[derive(Parser)]
#[command(name = "naga_oil_cli")]
#[command(author, version, about = None)]
#[command(
    about = "Construct standalone shaders from modules and definitions using naga_oil (https://github.com/bevyengine/naga_oil)"
)]
#[command(next_line_help = true)]
pub struct Cli {
    /// Disable checking invariance of variable names when regenerating wgsl or gsls from naga modules.
    /// It may be necessary to disable naga_oil's validation for features which are unsupported by naga::back::{glsl, wgsl}.
    #[arg(short, long, env = "NAGA_OIL_NO_VALIDATION", verbatim_doc_comment, action = clap::ArgAction::SetTrue)]
    no_validation: bool,

    /// Paths to check for included modules. Defaults to `.` if unspecified.
    ///
    /// If the argument resolves to a filename, the module will be made available to the composer.
    /// If the argument resolves to a folder, all shader files in the folder and subfolders are made available to the composer.
    /// If located modules contain a `#define_import_path` directive, this is used. Otherwise the module can be imported with the quoted filename relative to the path root, e.g. `#import "module.wgsl"` or `#import "subfolder/submodule.glsl"`
    /// This argument may be repeated to include multiple paths, or split with semicolons (`one.wgsl;two.wgsl`).
    #[arg(short, long, env = "NAGA_OIL_INCLUDE_PATH", verbatim_doc_comment)]
    include: Vec<String>,

    /// Shader definitions, specified as semicolon-separated names or name=value pairs.
    ///
    /// Raw names will be defined in the shader compilation with value Bool(true), useful for `#ifdef` and `#if def == true` directives.
    /// Other values can be specified with `name=value` where value should be
    /// - `true` or `false` for a bool value (e.g. `--defs MY_SETTING`)
    /// - a numeric literal for an i32 value (e.g. `--defs MY_NUMBER=-123`)
    /// - a non-negative numeric literal with a trailing `u` for a u32 value (e.g. `-d MY_NUMBER=123u`)
    /// This argument may be repeated to specify multiple defs, or split with semicolons (`-d ONE;TWO=123`).
    #[arg(short, long, env = "NAGA_OIL_DEFS", verbatim_doc_comment)]
    defs: Vec<String>,

    /// Additional shader definitions, added to or overwriting definitions provided with --defs. potentially useful for supplying additional defs when a base set is supplied via `--defs` or `NAGA_OIL_DEFS`
    #[arg(short, long, env = "NAGA_OIL_ADDITIONAL_DEFS", verbatim_doc_comment)]
    additional_defs: Vec<String>,

    /// The shader containing the target entry point
    shader: PathBuf,

    /// Output format. one of `WGSL`, `GLSL`, `NAGA` (serde_json serialized), `SPV`. If not specified, then if an ouptut filename is specified, attempts to determine the output based on the extension:
    ///
    /// `wgsl` => WGSL
    /// `frag`, `vert` => GLSL
    /// `bin`, `spv` => SPV
    /// `json` => NAGA
    ///
    /// Otherwise defaults to `WGSL`.
    ///
    /// For other output formats, you can pipe the output into naga's own cli tool
    #[arg(short, long, env = "NAGA_OIL_FORMAT", verbatim_doc_comment, value_parser = parse_format)]
    format: Option<OutputFormat>,

    /// Output file. if unspecified, output is written to stdout
    #[arg(short, long, env = "NAGA_OIL_OUTPUT")]
    output: Option<PathBuf>,
}

#[derive(Clone)]
enum OutputFormat {
    Wgsl,
    Glsl,
    Naga,
    Spirv,
}

fn parse_format(arg: &str) -> Result<OutputFormat, clap::Error> {
    match arg.trim().to_lowercase().as_str() {
        "wgsl" => Ok(OutputFormat::Wgsl),
        "glsl" => Ok(OutputFormat::Glsl),
        "naga" => Ok(OutputFormat::Naga),
        "spv" => Ok(OutputFormat::Spirv),
        _ => Err(clap::Error::new(clap::error::ErrorKind::InvalidValue)),
    }
}

fn gather_paths(args: &[String]) -> Vec<PathBuf> {
    if args.is_empty() {
        return vec![PathBuf::from_str(".").unwrap()];
    }
    let mut paths = Vec::default();
    for arg in args.iter().flat_map(|arg| arg.split(';')) {
        paths.push(PathBuf::from_str(arg).unwrap());
    }
    paths
}

fn parse_def_value(v: &str) -> ShaderDefValue {
    match v.trim().to_lowercase().as_str() {
        "true" => ShaderDefValue::Bool(true),
        "false" => ShaderDefValue::Bool(false),
        other => {
            if let Some(other) = other.strip_suffix('u') {
                ShaderDefValue::UInt(other.parse().unwrap())
            } else {
                ShaderDefValue::Int(other.parse().unwrap())
            }
        }
    }
}

fn gather_defs(args: &[String], add: &[String]) -> HashMap<String, ShaderDefValue> {
    let mut defs = HashMap::default();

    for def in args.iter().chain(add).flat_map(|def| def.split(';')) {
        if let Some((name, value)) = def.split_once('=') {
            defs.insert(name.to_owned(), parse_def_value(value));
        } else {
            defs.insert(def.to_owned(), ShaderDefValue::Bool(true));
        }
    }

    defs
}

fn shader_type(path: &Path) -> Option<ShaderType> {
    match path.extension() {
        Some(v) if v.to_string_lossy().to_lowercase() == "wgsl" => Some(ShaderType::Wgsl),
        Some(v) if v.to_string_lossy().to_lowercase() == "vert" => Some(ShaderType::GlslVertex),
        Some(v) if v.to_string_lossy().to_lowercase() == "frag" => Some(ShaderType::GlslFragment),
        _ => None,
    }
}

fn input_language(path: &Path) -> Option<ShaderLanguage> {
    shader_type(path).map(|ty| match ty {
        ShaderType::Wgsl => ShaderLanguage::Wgsl,
        ShaderType::GlslVertex | ShaderType::GlslFragment => ShaderLanguage::Glsl,
    })
}

fn main() {
    let cli = Cli::parse();

    // gather modules
    let mut include_paths = gather_paths(&cli.include);
    let mut includes = HashMap::new();

    while let Some(path) = include_paths.pop() {
        if path.is_dir() {
            let Ok(entries) = std::fs::read_dir(&path) else {
                panic!("failed to read directory {:?}", path);
            };
            include_paths.extend(entries.map(|e| e.unwrap().path()));
        } else {
            let Some(language) = input_language(&path) else {
                continue;
            };

            match std::fs::read_to_string(&path) {
                Err(e) => panic!("failed to read file `{}`: {e}", path.display()),
                Ok(source) => {
                    let (name, reqs, _) = naga_oil::compose::get_preprocessor_data(&source);
                    let name =
                        name.unwrap_or(format!("\"{}\"", path.to_string_lossy().into_owned()));
                    let reqs: HashSet<_> = reqs.into_iter().map(|req| req.import).collect();
                    if includes.contains_key(&name) {
                        eprintln!("warning: duplicate definition for `{name}`");
                    }
                    includes.insert(name, (reqs, path, language, source));
                }
            };
        }
    }

    let Ok(source) = std::fs::read_to_string(&cli.shader) else {
        panic!("failed to read main shader file {}", cli.shader.display());
    };

    let (_, reqs, _) = naga_oil::compose::get_preprocessor_data(&source);
    let mut reqs: HashSet<_> = reqs.into_iter().map(|req| req.import).collect();

    let capabilities = Capabilities::all();

    let mut composer = if cli.no_validation {
        Composer::non_validating()
    } else {
        Composer::default()
    }
    .with_capabilities(capabilities);

    // add required imports
    while !reqs.is_empty() {
        let mut next_reqs: HashSet<String> = HashSet::default();
        for req in reqs.iter() {
            if !composer.contains_module(req) {
                if let Some((subreqs, path, language, source)) = includes.get(req) {
                    if subreqs
                        .iter()
                        .all(|subreq| composer.contains_module(subreq))
                    {
                        eprintln!("adding module {req}");
                        composer
                            .add_composable_module(ComposableModuleDescriptor {
                                source,
                                file_path: &path.to_string_lossy(),
                                language: *language,
                                as_name: Some(req.clone()),
                                ..Default::default()
                            })
                            .unwrap();
                    }
                    next_reqs.extend(
                        subreqs
                            .iter()
                            .filter(|r| !composer.contains_module(r))
                            .cloned(),
                    );
                    next_reqs.insert(req.clone());
                } else {
                    panic!("required import {} not found in included paths", req);
                }
            }
        }

        if next_reqs == reqs {
            panic!("circular dependency: {:?}", next_reqs)
        }
        reqs = next_reqs;
    }

    // run composer
    let composed = composer.make_naga_module(NagaModuleDescriptor {
        source: &source,
        file_path: &cli.shader.to_string_lossy(),
        shader_type: shader_type(&cli.shader)
            .unwrap_or_else(|| panic!("input shader must have extension `wgsl`, `vert` or `frag`")),
        shader_defs: gather_defs(&cli.defs, &cli.additional_defs),
        ..Default::default()
    });

    if let Err(e) = composed {
        let err_str = e.emit_to_string(&composer);
        eprintln!("{err_str}");
        exit(1)
    }

    let composed = composed.unwrap();

    // output
    let mut target: Box<dyn Write> = cli
        .output
        .as_ref()
        .map(|path| Box::new(std::fs::File::create(path).unwrap()) as Box<dyn Write>)
        .unwrap_or(Box::new(stdout()));

    let output_format = cli.format.unwrap_or_else(|| {
        cli.output
            .as_ref()
            .and_then(|path| path.extension().map(|o| o.to_string_lossy().into_owned()))
            .and_then(|ext| match ext.trim().to_lowercase().as_str() {
                "wgsl" => Some(OutputFormat::Wgsl),
                "frag" | "vert" => Some(OutputFormat::Glsl),
                "json" => Some(OutputFormat::Naga),
                "spv" | "bin" => Some(OutputFormat::Spirv),
                _ => None,
            })
            .unwrap_or(OutputFormat::Wgsl)
    });

    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), capabilities)
        .validate(&composed)
        .map_err(ComposerErrorInner::HeaderValidationError)
        .unwrap();

    let shader_stage = composed.entry_points.first().unwrap().stage;
    let entry_point = composed.entry_points.first().unwrap().name.clone();

    match output_format {
        OutputFormat::Wgsl => {
            let output = naga::back::wgsl::write_string(
                &composed,
                &info,
                naga::back::wgsl::WriterFlags::EXPLICIT_TYPES,
            )
            .unwrap();
            target.write_all(output.as_bytes()).unwrap();
        }
        OutputFormat::Glsl => {
            let mut string = String::new();
            let options = naga::back::glsl::Options {
                version: naga::back::glsl::Version::Desktop(450),
                writer_flags: naga::back::glsl::WriterFlags::INCLUDE_UNUSED_ITEMS,
                ..Default::default()
            };
            let pipeline_options = naga::back::glsl::PipelineOptions {
                shader_stage,
                entry_point,
                multiview: None,
            };
            let mut writer = naga::back::glsl::Writer::new(
                &mut string,
                &composed,
                &info,
                &options,
                &pipeline_options,
                naga::proc::BoundsCheckPolicies::default(),
            )
            .map_err(ComposerErrorInner::GlslBackError)
            .unwrap();

            writer
                .write()
                .map_err(ComposerErrorInner::GlslBackError)
                .unwrap();
            target.write_all(string.as_bytes()).unwrap();
        }
        OutputFormat::Spirv => {
            let vec = naga::back::spv::write_vec(
                &composed,
                &info,
                &naga::back::spv::Options::default(),
                Some(&naga::back::spv::PipelineOptions {
                    shader_stage,
                    entry_point,
                }),
            )
            .unwrap();
            for long in vec.iter() {
                for b in long.to_be_bytes() {
                    target.write_all(&[b]).unwrap();
                }
            }
        }
        OutputFormat::Naga => target
            .write_all(&serde_json::to_vec(&composed).unwrap())
            .unwrap(),
    }
}
