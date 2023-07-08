Runs a set of commands as child-processes, capturing their output as files into a zip archive.

Because temporary files are annoying.

Usage: cmd2zip.exe [OPTIONS] [COMMANDS]...

Arguments:
  [COMMANDS]...
          The commands to run; allows for glob-expansion, even on Windows!

Options:
  -o, --output <OUTPUT>
          The name/path of the zip archive to output to

          [default: output.zip]

      --cmd-prefix <PREFIX>
          Prefix to be prepended to all commands

      --cmd-postfix <POSTFIX>
          Postfix to be appended to all commands

  -p, --name-pattern <NAME_PATTERN>
          Regex pattern to extract a filename from each command.

          Internally uses the <https://docs.rs/regex/latest/regex/index.html#syntax> crate.

          A typical pattern would be `([\w-]+)\.EXT$`.

  -r, --name-replace <NAME_REPLACE>
          Regex replacement string.

          If this option is not set, the *entire* matched pattern is used.

          - `$N` is replaced with the matching positional capture.

          - `$NAME` is replaced with the matching named capture.

          A typical replacement would be `$1.EXT`.

      --name-prefix <NAME_PREFIX>
          Prefix to prepend to all generated filenames.

          Applied AFTER regex match/replace.

      --name-postfix <NAME_POSTFIX>
          Postfix to append to all generated filenames.

          Applied AFTER regex match/replace and prefix.

  -t, --threads <THREADS>
          The number of child processes to run in parallel; default is 0 for all cores

          [env: RAYON_NUM_THREADS=]
          [default: 0]

  -a, --append
          Append to the zip archive specified by `output`, instead of replacing it

  -h, --help
          Print help (see a summary with '-h')