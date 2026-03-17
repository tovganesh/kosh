use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;
use crate::error::{ShellError, ShellResult};
use crate::types::Environment;

pub struct CommandProcessor {
    // Basic command processor - will be enhanced in later tasks
}

impl CommandProcessor {
    pub fn new() -> Self {
        Self {}
    }
    
    pub fn process_command(&mut self, command_line: &str) -> ShellResult<String> {
        let command_line = command_line.trim();
        
        if command_line.is_empty() {
            return Ok(String::new());
        }
        
        let parts: Vec<&str> = command_line.split_whitespace().collect();
        let command = parts[0];
        let args = &parts[1..];
        
        match command {
            "help" => self.cmd_help(),
            "echo" => self.cmd_echo(args),
            "ps" => self.cmd_ps(),
            "ls" => self.cmd_ls(args),
            "cat" => self.cmd_cat(args),
            "mkdir" => self.cmd_mkdir(args),
            "rmdir" => self.cmd_rmdir(args),
            "touch" => self.cmd_touch(args),
            "rm" => self.cmd_rm(args),
            "pwd" => self.cmd_pwd(),
            "cd" => self.cmd_cd(args),
            "clear" => self.cmd_clear(),
            "exit" => self.cmd_exit(),
            "shutdown" => self.cmd_shutdown(),
            _ => Err(ShellError::InvalidCommand(command.to_string())),
        }
    }
    
    /// Process environment-related commands that need access to the Environment.
    /// Returns Some(output) if the command was handled, None if it's not an env command.
    pub fn process_env_command(command: &str, args: &[&str], env: &mut Environment) -> Option<ShellResult<String>> {
        match command {
            "export" => Some(Self::cmd_export(args, env)),
            "unset" => Some(Self::cmd_unset(args, env)),
            "env" => Some(Self::cmd_env(env)),
            _ => None,
        }
    }

    /// export command: set or display environment variables.
    /// Usage: export NAME=VALUE  — set a variable
    ///        export             — list all variables
    fn cmd_export(args: &[&str], env: &mut Environment) -> ShellResult<String> {
        if args.is_empty() {
            // Display all variables in export format
            let mut output = String::new();
            for (key, value) in env.iter() {
                output.push_str("export ");
                output.push_str(key);
                output.push_str("=\"");
                output.push_str(value);
                output.push_str("\"\n");
            }
            if output.ends_with('\n') {
                output.pop();
            }
            return Ok(output);
        }

        for arg in args {
            match Environment::parse_assignment(arg) {
                Some((name, value)) => {
                    env.set_var(name, value);
                }
                None => {
                    // If no '=', just check if the variable name is valid
                    if !Environment::is_valid_var_name(arg) {
                        return Err(ShellError::InvalidArguments(
                            format!("export: '{}': not a valid identifier", arg),
                        ));
                    }
                    // export without value — variable already exists, nothing to do
                }
            }
        }

        Ok(String::new())
    }

    /// unset command: remove environment variables.
    /// Usage: unset NAME [NAME ...]
    fn cmd_unset(args: &[&str], env: &mut Environment) -> ShellResult<String> {
        if args.is_empty() {
            return Err(ShellError::InvalidArguments(
                "Usage: unset NAME [NAME ...]".to_string(),
            ));
        }

        for arg in args {
            if !Environment::is_valid_var_name(arg) {
                return Err(ShellError::InvalidArguments(
                    format!("unset: '{}': not a valid identifier", arg),
                ));
            }
            env.unset_var(arg);
        }

        Ok(String::new())
    }

    /// env command: list all environment variables.
    /// Usage: env
    fn cmd_env(env: &Environment) -> ShellResult<String> {
        Ok(env.format_all())
    }
    
    fn cmd_help(&self) -> ShellResult<String> {
        let help_text = "Available commands:\n\
            help     - Show this help message\n\
            echo     - Echo arguments to output\n\
            ps       - List running processes\n\
            ls       - List directory contents\n\
            cat      - Display file contents\n\
            mkdir    - Create directory\n\
            rmdir    - Remove directory\n\
            touch    - Create empty file\n\
            rm       - Remove file\n\
            pwd      - Print working directory\n\
            cd       - Change directory\n\
            export   - Set/display environment variables\n\
            unset    - Remove environment variables\n\
            env      - List all environment variables\n\
            clear    - Clear screen\n\
            exit     - Exit shell\n\
            shutdown - Shutdown system";
        
        Ok(String::from(help_text))
    }
    
    fn cmd_echo(&self, args: &[&str]) -> ShellResult<String> {
        Ok(args.join(" "))
    }
    
    fn cmd_ps(&self) -> ShellResult<String> {
        // In a real implementation, this would query the kernel for process list
        Ok(String::from("PID  NAME\n1    init\n2    fs-service\n3    driver-manager\n4    shell"))
    }
    
    fn cmd_ls(&self, args: &[&str]) -> ShellResult<String> {
        let path = if args.is_empty() { "." } else { args[0] };
        
        // In a real implementation, this would use file system service
        match path {
            "." | "/" => Ok(String::from("bin/\ndev/\netc/\nhome/\nlib/\ntmp/\nusr/\nvar/")),
            "/bin" => Ok(String::from("shell\nls\ncat\necho")),
            "/dev" => Ok(String::from("console\nkeyboard\nmouse\nstorage")),
            _ => Ok(String::from("Directory listing not available")),
        }
    }
    
    fn cmd_cat(&self, args: &[&str]) -> ShellResult<String> {
        if args.is_empty() {
            return Err(ShellError::InvalidArguments("Usage: cat <filename>".to_string()));
        }
        
        // In a real implementation, this would read from file system service
        Ok(format!("Contents of {} (not implemented)", args[0]))
    }
    
    fn cmd_mkdir(&self, args: &[&str]) -> ShellResult<String> {
        if args.is_empty() {
            return Err(ShellError::InvalidArguments("Usage: mkdir <directory>".to_string()));
        }
        
        // In a real implementation, this would create directory via file system service
        Ok(format!("Created directory: {} (not implemented)", args[0]))
    }
    
    fn cmd_rmdir(&self, args: &[&str]) -> ShellResult<String> {
        if args.is_empty() {
            return Err(ShellError::InvalidArguments("Usage: rmdir <directory>".to_string()));
        }
        
        // In a real implementation, this would remove directory via file system service
        Ok(format!("Removed directory: {} (not implemented)", args[0]))
    }
    
    fn cmd_touch(&self, args: &[&str]) -> ShellResult<String> {
        if args.is_empty() {
            return Err(ShellError::InvalidArguments("Usage: touch <filename>".to_string()));
        }
        
        // In a real implementation, this would create file via file system service
        Ok(format!("Created file: {} (not implemented)", args[0]))
    }
    
    fn cmd_rm(&self, args: &[&str]) -> ShellResult<String> {
        if args.is_empty() {
            return Err(ShellError::InvalidArguments("Usage: rm <filename>".to_string()));
        }
        
        // In a real implementation, this would remove file via file system service
        Ok(format!("Removed file: {} (not implemented)", args[0]))
    }
    
    fn cmd_pwd(&self) -> ShellResult<String> {
        // In a real implementation, this would track current working directory
        Ok(String::from("/"))
    }
    
    fn cmd_cd(&self, args: &[&str]) -> ShellResult<String> {
        let path = if args.is_empty() { "/" } else { args[0] };
        
        // In a real implementation, this would change working directory
        Ok(format!("Changed directory to: {} (not implemented)", path))
    }
    
    fn cmd_clear(&self) -> ShellResult<String> {
        // In a real implementation, this would clear the screen
        Ok(String::from("\n\n\n\n\n\n\n\n\n\n--- Screen cleared ---"))
    }
    
    fn cmd_exit(&self) -> ShellResult<String> {
        Ok(String::from("Goodbye!"))
    }
    
    fn cmd_shutdown(&self) -> ShellResult<String> {
        // In a real implementation, this would send shutdown signal to init
        Ok(String::from("System shutdown requested (not implemented)"))
    }
}