Construct standalone shaders from modules and definitions using naga_oil (https://github.com/bevyengine/naga_oil)

Usage: naga_oil_cli.exe [OPTIONS] <SHADER>

Arguments:
  <SHADER>
          The shader containing the target entry point

Options:
  -n, --no-validation
          Disable checking invariance of variable names when regenerating wgsl or gsls from naga modules.
          It may be necessary to disable naga_oil's validation for features which are unsupported by naga::back::{glsl, wgsl}.

          [env: NAGA_OIL_NO_VALIDATION=]

  -i, --include <INCLUDE>
          Paths to check for included modules. Defaults to `.` if unspecified.

          If the argument resolves to a filename, the module will be made available to the composer.
          If the argument resolves to a folder, all shader files in the folder and subfolders are made available to the composer.
          If located modules contain a `#define_import_path` directive, this is used. Otherwise the module can be imported with the quoted filename relative to the path root, e.g. `#import "module.wgsl"` or `#import "subfolder/submodule.glsl"`
          This argument may be repeated to include multiple paths, or split with semicolons (`one.wgsl;two.wgsl`).

          [env: NAGA_OIL_INCLUDE_PATH=]

  -d, --defs <DEFS>
          Shader definitions, specified as semicolon-separated names or name=value pairs.

          Raw names will be defined in the shader compilation with value Bool(true), useful for `#ifdef` and `#if def == true` directives.
          Other values can be specified with `name=value` where value should be
          - `true` or `false` for a bool value (e.g. `--defs MY_SETTING`)
          - a numeric literal for an i32 value (e.g. `--defs MY_NUMBER=-123`)
          - a non-negative numeric literal with a trailing `u` for a u32 value (e.g. `-d MY_NUMBER=123u`)
          This argument may be repeated to specify multiple defs, or split with semicolons (`-d ONE;TWO=123`).

          [env: NAGA_OIL_DEFS=]

  -a, --additional-defs <ADDITIONAL_DEFS>
          Additional shader definitions, added to or overwriting definitions provided with --defs. potentially useful for supplying additional defs when a base set is supplied via `--defs` or `NAGA_OIL_DEFS`

          [env: NAGA_OIL_ADDITIONAL_DEFS=]

  -f, --format <FORMAT>
          Output format. one of `WGSL`, `GLSL`, `NAGA` (serde_json serialized), `SPV`. If not specified, then if an ouptut filename is specified, attempts to determine the output based on the extension:

          `wgsl` => WGSL 
          `frag`, `vert` => GLSL 
          `bin`, `spv` => SPV 
          `json` => NAGA

          Otherwise defaults to `WGSL`.

          For other output formats, you can pipe the output into naga's own cli tool

          [env: NAGA_OIL_FORMAT=]

  -o, --output <OUTPUT>
          Output file. if unspecified, output is written to stdout

          [env: NAGA_OIL_OUTPUT=]

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
