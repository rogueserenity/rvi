fn main() {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();

    let mut readonly = false;
    let mut startup_commands: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    // -r with no file: list recoverable files and exit.
    // -r {file}: recover that file.
    let mut recover_file: Option<Option<String>> = None; // None = not set; Some(None) = list; Some(Some(f)) = recover f
    let mut tagstring: Option<String> = None;

    let mut i = 0;
    while i < raw_args.len() {
        let arg = &raw_args[i];
        if arg == "-R" {
            readonly = true;
        } else if arg == "-t" {
            i += 1;
            if let Some(ts) = raw_args.get(i) {
                tagstring = Some(ts.clone());
            } else {
                eprintln!("rvi: -t requires an argument");
                std::process::exit(1);
            }
        } else if arg == "-r" {
            // If a next argument exists, it is always the file to recover
            // (POSIX: -r file). Only bare `-r` with no following argument
            // enters list mode. We do NOT skip arguments that start with '-'
            // since filenames can legitimately begin with that character.
            if let Some(next) = raw_args.get(i + 1) {
                recover_file = Some(Some(next.clone()));
                i += 1; // consume the filename
            } else {
                recover_file = Some(None); // list mode: print recoverable files and exit
            }
        } else if arg == "-c" {
            i += 1;
            if let Some(cmd) = raw_args.get(i) {
                startup_commands.push(cmd.clone());
            } else {
                eprintln!("rvi: -c requires an argument");
                std::process::exit(1);
            }
        } else if let Some(cmd) = arg.strip_prefix('+') {
            // +{cmd} or +{linenum}: treat a bare number as a :goto-line command.
            if cmd.is_empty() || cmd.chars().all(|c| c.is_ascii_digit()) {
                if cmd.is_empty() {
                    startup_commands.push("$".to_string());
                } else {
                    startup_commands.push(cmd.to_string());
                }
            } else {
                startup_commands.push(cmd.to_string());
            }
        } else if arg.starts_with('-') {
            eprintln!("rvi: unknown option: {}", arg);
            std::process::exit(1);
        } else {
            files.push(arg.clone());
        }
        i += 1;
    }

    // -r with no file: list recoverable files and exit without starting the editor.
    if recover_file == Some(None) {
        let entries = rvi::Editor::list_preserve_files();
        if entries.is_empty() {
            println!("No recoverable files found in /tmp.");
        } else {
            println!("Recoverable files:");
            for entry in entries {
                println!("{}", entry);
            }
        }
        return;
    }

    let mut editor = match rvi::Editor::new() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    if readonly {
        editor.set_readonly(true);
    }

    // -r {file}: open the file first (so filename/path are set), then recover.
    if let Some(Some(ref fname)) = recover_file {
        // Open the original file to set document.filename / file_path so that
        // a subsequent :w targets the right path. This is best-effort: the file
        // may not exist on disk (e.g. it was deleted after the crash), which is
        // a normal recovery scenario.
        if let Err(e) = editor.open_file(fname) {
            eprintln!(
                "rvi: warning: could not open original file \"{}\": {}",
                fname, e
            );
        }
        if let Err(e) = editor.recover_buffer(Some(fname.clone())) {
            eprintln!("rvi: {}", e);
            std::process::exit(1);
        }
    } else if let Some(ref ts) = tagstring {
        // -t takes precedence over positional files; skip set_arg_list.
        // Failure is fatal: print to stderr and exit as POSIX vi requires.
        if let Err(e) = editor.execute_tag_jump(ts) {
            eprintln!("rvi: {}", e);
            std::process::exit(1);
        }
    } else if !files.is_empty() {
        if let Err(e) = editor.set_arg_list(files) {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }

    if !startup_commands.is_empty() {
        editor.execute_startup_commands(&startup_commands);
    }

    if let Err(e) = editor.run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
