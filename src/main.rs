use std::{
    fs::File,
    path::PathBuf,
    io::{Write, Seek, BufRead},
    process::Command,
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicUsize, Ordering}
    }
};

use clap::Parser;
use regex::Regex;
use rayon::ThreadPoolBuilder;
use zip::{ZipWriter, write::FileOptions};

/// # cmd2zip
/// 
/// Runs a set of commands as child-processes, capturing their output as files into a zip archive... because temporary files are annoying!
/// 
/// The names of the resulting files are either incrementing numbers, or a regex match/expand over the command.
/// 
/// ## Notes
/// 
/// - Commands starting with `#` are printed to the console, without being run.
/// 
/// - If a command fails, it's output is written to the archive as `.err`-file.
/// 
/// - On windows, backward-slashes within glob-expanded commands become forward-slashes.
/// 
/// - Finished commands are listed via stdout; anything else goes to stderr.
/// 
/// ## Example
/// 
/// Generating PNG images by globbing SVGs into resvg:
/// 
/// ```sh
/// cmd2zip -o "icons.zip" -p '(?P<name>[\w\-]+)\.svg$' -r '$name.png' --cmd-prefix "resvg -w 128 -h 128" --cmd-postfix " -c" ./icons/*.svg
/// ```
/// 
#[derive(Debug, Parser)]
struct CmdToZip {
    /// Also pull commands from the given file or stdin (via `-`).
    #[arg(short = 'i', long = "input")]
    input: Option<PathBuf>,
    
    /// The name/path of the zip archive to output to.
    /// 
    /// Location MUST be writable.
    #[arg(short = 'o', long = "output", default_value = "output.zip")]
    output: PathBuf,
    
    /// Prefix to be prepended to all commands.
    /// 
    /// Does NOT partake in name generation.
    #[arg(long = "cmd-prefix")]
    prefix: Option<String>,
    
    /// Postfix to be appended to all commands.
    /// 
    /// Does NOT partake in name generation.
    #[arg(long = "cmd-postfix")]
    postfix: Option<String>,
    
    /// Regex pattern to extract a filename from each command.
    /// 
    /// Internally uses the <https://docs.rs/regex/latest/regex/index.html#syntax> crate.
    /// 
    /// A typical pattern would be `([\w-]+)\.EXT$`.
    #[arg(short = 'p', long = "name-pattern")]
    name_pattern: Option<Regex>,
    
    /// Regex replacement expansion string.
    /// 
    /// If this option is not set, the *entire* matched pattern is used.
    /// 
    /// - `$N` is replaced with the matching positional capture.
    /// 
    /// - `$NAME` is replaced with the matching named capture.
    /// 
    /// A typical replacement would be `$1.EXT`.
    #[arg(short = 'r', long = "name-replace", requires = "name_pattern")]
    name_replace: Option<String>,
    
    /// Prefix to prepend to all generated filenames.
    /// 
    /// Applied AFTER regex match/replace.
    #[arg(long = "name-prefix")]
    name_prefix: Option<String>,
    
    /// Postfix to append to all generated filenames.
    /// 
    /// Applied AFTER name prefix.
    #[arg(long = "name-postfix")]
    name_postfix: Option<String>,
    
    /// The number of child processes to run in parallel; default is 0 for all cores.
    #[arg(short = 't', long = "threads", env = "RAYON_NUM_THREADS", default_value_t = 0)]
    threads: usize,
    
    /// The maximum number of commands to run.
    #[arg(short = 'l', long = "limit")]
    limit: Option<usize>,
    
    /// Append to the zip archive specified by `output`, instead of replacing it.
    #[arg(short, long = "append", default_value = "false")]
    append: bool,
    
    /// Instead of running and capturing commands, write the commands themself to the archive.
    #[arg(short = 'd', long = "dry-run", default_value = "false")]
    dry: bool,
    
    /// The commands to run; allows for glob-expansion, even on Windows!
    #[arg(action = clap::ArgAction::Append)]
    commands: Vec<String>
}


fn main() {
    let args = wild::args_os();
    let mut args = CmdToZip::parse_from(args);
    
    let prefix = Arc::new(args.prefix.map(|s| s + " ").unwrap_or_else(||String::default()));
    let postfix = Arc::new(args.postfix.unwrap_or_else(||String::default()));
    
    let pool = ThreadPoolBuilder::new()
        .num_threads(0)
        .build()
        .expect("failed to build thread-pool");
    
    let mut name_gen: Arc<dyn Fn(&str) -> String + Send + Sync> = match (args.name_pattern, args.name_replace) {
        (Some(r), None) => {
            eprintln!("-- Using regex-based name generator without replacement: {}", r.as_str());
            Arc::new(move |c: &str| {
                r.find(c).expect("failed to capture").as_str().to_string()
            })
        },
        (Some(r), Some(p)) => {
            eprintln!("-- Using regex-based name generator with replacement expansion: {} / {}", r.as_str(), p.as_str());
            Arc::new(move |c: &str| {
                let captures = r.captures(c).expect("failed to capture pattern");
                let mut name = String::with_capacity(16);
                captures.expand(&p, &mut name);
                name
            })
        },
        (None, Some(_)) => panic!("cannot specify replacement without regex"),
        (None, None) => {
            eprintln!("-- Using numeric name generator.");
            let counter = Arc::new(AtomicUsize::new(0));
            Arc::new(
                move |_c: &str| {
                    let num = counter.fetch_add(1, Ordering::Relaxed);
                    format!("{}", num)
                }
            )
        },
    };
    
    if let Some(np) = args.name_prefix {
        let old = name_gen.clone();
        name_gen = Arc::new(move |c| {
            format!("{}{}", np, (old)(c))
        });
    }
    
    if let Some(np) = args.name_postfix {
        let old = name_gen.clone();
        name_gen = Arc::new(move |c| {
            format!("{}{}", (old)(c), np)
        });
    }
    
    let archive = if args.append {
        let archive = File::options().write(true).append(true).open(&args.output).unwrap();
        let archive = ZipWriter::new_append(archive).expect("failed to open archive for appending");
        archive
    } else {
        let archive = File::create(&args.output).unwrap();
        let archive = ZipWriter::new(archive);
        archive
    };
    
    let archive = Mutex::new(archive);
    let archive = Arc::new(archive);
    
    let commands: Box<dyn Iterator<Item = String>> = if let Some(input) = args.input {
        Box::new(open_input(input).chain(args.commands))
    } else {
        Box::new(args.commands.into_iter())
    };
    
    let tasks = Arc::new(AtomicUsize::new(0));
    
    for command in commands {
        
        if let Some(limit) = &mut args.limit {
            *limit -= 1;
            if *limit == 0 {
                eprintln!("!! Reached command limit");
                break;
            }
        }
        
        let tasks = tasks.clone();
        let archive = archive.clone();
        let prefix = prefix.clone();
        let postfix = postfix.clone();
        let name_gen = name_gen.clone();
        
        // Ignore commands starting with a hashtag
        if command.starts_with('#') {
            eprintln!("## {}", &command[1..]);
            continue;
        }
        
        tasks.fetch_add(1, Ordering::Relaxed);
        
        pool.spawn(move || {
            // FIXME: The wild-crate emits backward-slashes on windows, which may break some commands.
            // TODO: Perhaps make this an option?
            #[cfg(target_os = "windows")]
            let command = command.replace("\\", "/");
            
            let full_command = format!("{prefix}{command}{postfix}");
            
            // Generate file-name!
            let mut name = (name_gen)(&command);
            
            // --- Build the command and run the child-process
            
            // Note: This blocks until the child finishes, ON PURPOSE.
            let (status, mut stdout, mut stderr) = if ! args.dry {
                let output = build_command(&full_command).output().expect("failed to run command");
                (output.status.success(), output.stdout, output.stderr)
            } else {
                name = name + ".txt";
                (true, full_command.as_bytes().to_vec(), vec![])
            };
            
            // --- Process output...
            let mut using = "stdout";
            
            if stdout.len() == 0 {
                eprintln!("!! Command had no stdout, writing stderr instead: {full_command}");
                std::mem::swap(&mut stdout, &mut stderr);
                using = "stderr";
            }
            
            if !status {
                eprintln!("!! Command failed: {full_command}\n{}", std::str::from_utf8(&stdout).unwrap());
                name = name + ".err";
            }
            
            println!("`{name}` << {} bytes from {using} << `{full_command}`", stdout.len());
            append_to_archive(&archive, &name, &stdout);
            
            tasks.fetch_sub(1, Ordering::Relaxed);
        });
    }
    
    eprintln!("-- Waiting for all children to finish...");
    
    // Now wait for all children to finish...
    while tasks.load(Ordering::Relaxed) != 0 {}
    
    let mut a = archive.lock().expect("failed to re-acquire archive writer");
    a.finish().expect("failed to finish writing archive");
    drop(a);
    drop(archive);
    
    eprintln!("-- Done!");
}

fn open_input(input: PathBuf) -> Box<dyn std::iter::Iterator<Item = String>> {
    if input == PathBuf::from("-") {
        Box::new(
            std::io::stdin()
            .lines()
            .flatten()
        )
    } else {
        Box::new(
            std::io::BufReader::new(std::fs::File::open(input).expect("failed to open input file"))
            .lines()
            .flatten()
        )
    }
}

fn build_command(command: &str) -> Command {
    let split_command = shlex::split(command).expect("failed to shlex command");
    let mut child = Command::new(&split_command[0]);
    child.args(&split_command[1..]);
    child
}

fn append_to_archive(archive: &Mutex<ZipWriter<impl Write + Seek>>, file_name: &str, file_content: &[u8]) {
    let mut a = archive.lock().expect("failed to lock mutex");
    a.start_file(file_name, FileOptions::default()).expect("failed to start file");
    a.write_all(file_content).expect("failed to write file");
    a.flush().expect("failed to flush archive writer");
}
