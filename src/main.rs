use std::{
    fs::File,
    path::PathBuf,
    io::{Write, Seek},
    process::Command,
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicUsize, Ordering}
    }
};

use clap::Parser;
use regex::Regex;
use rayon::{prelude::*, ThreadPoolBuilder};
use zip::{ZipWriter, write::FileOptions};

/// Runs a set of commands as child-processes, capturing their output as files into a zip archive.
/// 
/// Because temporary files are annoying.
#[derive(Debug, Parser)]
struct CmdToZip {
    /// The name/path of the zip archive to output to.
    #[arg(short = 'o', long = "output", default_value = "output.zip")]
    output: PathBuf,
    
    /// Prefix to be prepended to all commands.
    #[arg(long = "cmd-prefix")]
    prefix: Option<String>,
    
    /// Postfix to be appended to all commands.
    #[arg(long = "cmd-postfix")]
    postfix: Option<String>,
    
    /// Regex pattern to extract a filename from each command.
    /// 
    /// Internally uses the <https://docs.rs/regex/latest/regex/index.html#syntax> crate.
    /// 
    /// A typical pattern would be `([\w-]+)\.EXT$`.
    #[arg(short = 'p', long = "name-pattern")]
    name_pattern: Option<Regex>,
    
    /// Regex replacement string.
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
    /// Applied AFTER regex match/replace and prefix.
    #[arg(long = "name-postfix")]
    name_postfix: Option<String>,
    
    /// The number of child processes to run in parallel; default is 0 for all cores.
    #[arg(short = 't', long = "threads", env = "RAYON_NUM_THREADS", default_value_t = 0)]
    threads: usize,
    
    /// Append to the zip archive specified by `output`, instead of replacing it.
    #[arg(short, long = "append")]
    append: bool,
    
    // TODO: Implement dry-run option?
    // /// Dry-run
    // #[arg(long)]
    // dry: bool,
    
    /// The commands to run; allows for glob-expansion, even on Windows!
    #[arg(action = clap::ArgAction::Append)]
    commands: Vec<String>
}


fn main() {
    let args = wild::args_os();
    let args = CmdToZip::parse_from(args);
    
    let prefix = args.prefix.map(|s| s + " ").unwrap_or_else(||String::default());
    let postfix = args.postfix.map(|s| s + " ").unwrap_or_else(||String::default());
    
    let _pool = ThreadPoolBuilder::new()
        .num_threads(0)
        .build_global()
        .expect("failed to build thread-pool");
    
    let mut name_gen: Box<dyn Fn(&str) -> String + Sync> = match (&args.name_pattern, &args.name_replace) {
        (Some(r), None) => {
            eprintln!("Using regex-based name generator with default replacement ($0).");
            Box::new(move |c: &str| {
                r.replace_all(c, "$0").to_string()
            })
        },
        (Some(r), Some(p)) => {
            eprintln!("Using regex-based name generator with custom replacement.");
            Box::new(move |c: &str| {
                r.replace_all(c, p).to_string()
            })
        },
        (None, Some(_)) => panic!("cannot specify replacement without regex"),
        (None, None) => {
            eprintln!("Using numeric name generator.");
            let counter = Arc::new(AtomicUsize::new(0));
            Box::new(
                move |_c: &str| {
                    let num = counter.fetch_add(1, Ordering::Relaxed);
                    format!("{}", num)
                }
            )
        },
    };
    
    if let Some(np) = args.name_prefix {
        let old = name_gen;
        name_gen = Box::new(move |c| {
            format!("{}{}", np, (old)(c))
        });
    }
    
    if let Some(np) = args.name_postfix {
        let old = name_gen;
        name_gen = Box::new(move |c| {
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
    
    (args.commands).as_slice().par_iter().enumerate().for_each(|(_i, command)| {
        // FIXME: The wild-crate emits backward-slashes on windows, which may break some commands.
        // TODO: Perhaps make this an option?
        #[cfg(target_os = "windows")]
        let command = command.replace("\\", "/");
        
        let full_command = format!("{prefix}{command}{postfix}");
        
        // Generate file-name!
        let mut name = (name_gen)(&command);
        
        // Print command and the generated name.
        println!("{full_command} >> {name}");
        
        // --- Build the command and run the child-process
        
        // Note: This blocks until the child finishes, ON PURPOSE.
        let child = build_command(&full_command).output().expect("failed to run command");
        
        // --- Process output...
        
        let mut stdout = child.stdout;
        let mut stderr = child.stderr;
        let mut using = "stdout";
        
        if stdout.len() == 0 {
            eprintln!("Command had no stdout, writing stderr instead: {full_command}");
            std::mem::swap(&mut stdout, &mut stderr);
            using = "stderr";
        }
        
        if !child.status.success() {
            eprintln!("!! Command failed: {full_command}\n{}", std::str::from_utf8(&stdout).unwrap());
            name = name + ".err";
        }
        
        println!("{name} << {} bytes from {using} << {full_command} ", stdout.len());
        append_to_archive(&archive, &name, &stdout);
    });
    
    let mut a = archive.lock().expect("failed to re-acquire archive writer");
    a.finish().expect("failed to finish writing archive");
    drop(a);
    drop(archive);
    
    println!("Done!");
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
