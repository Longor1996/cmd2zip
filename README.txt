# cmd2zip

Runs a set of commands as child-processes, capturing their output as files into a zip archive... because temporary files are annoying!

The names of the resulting files are either incrementing numbers, or a regex match/expand over the command.

## Notes

- Commands starting with `#` are printed to the console, without being run.

- If a command fails, it's output is written to the archive as `.err`-file.

- On windows, backward-slashes within glob-expanded commands become forward-slashes.

- Finished commands are listed via stdout; anything else goes to stderr.

## Example

Generating PNG images by globbing SVGs into resvg:

```sh cmd2zip -o "icons.zip" -p '(?P<name>[\w\-]+)\.svg$' -r '$name.png' --cmd-prefix "resvg -w 128 -h 128" --cmd-postfix " -c" ./icons/*.svg ```

Usage: cmd2zip.exe [OPTIONS] [COMMANDS]...

Arguments:
  [COMMANDS]...
          The commands to run; allows for glob-expansion, even on Windows!

Options:
  -i, --input <INPUT>
          Also pull commands from the given file or stdin (via `-`)

  -o, --output <OUTPUT>
          The name/path of the zip archive to output to.

          Location MUST be writable.

          [default: output.zip]

      --cmd-prefix <PREFIX>
          Prefix to be prepended to all commands.

          Does NOT partake in name generation.

      --cmd-postfix <POSTFIX>
          Postfix to be appended to all commands.

          Does NOT partake in name generation.

  -p, --name-pattern <NAME_PATTERN>
          Regex pattern to extract a filename from each command.

          Internally uses the <https://docs.rs/regex/latest/regex/index.html#syntax> crate.

          A typical pattern would be `([\w-]+)\.EXT$`.

  -r, --name-replace <NAME_REPLACE>
          Regex replacement expansion string.

          If this option is not set, the *entire* matched pattern is used.

          - `$N` is replaced with the matching positional capture.

          - `$NAME` is replaced with the matching named capture.

          A typical replacement would be `$1.EXT`.

      --name-prefix <NAME_PREFIX>
          Prefix to prepend to all generated filenames.

          Applied AFTER regex match/replace.

      --name-postfix <NAME_POSTFIX>
          Postfix to append to all generated filenames.

          Applied AFTER name prefix.

  -t, --threads <THREADS>
          The number of child processes to run in parallel; default is 0 for all cores

          [env: RAYON_NUM_THREADS=]
          [default: 0]

  -l, --limit <LIMIT>
          The maximum number of commands to run

  -a, --append
          Append to the zip archive specified by `output`, instead of replacing it

  -d, --dry-run
          Instead of running and capturing commands, write the commands themself to the archive

  -h, --help
          Print help (see a summary with '-h')
