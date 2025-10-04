#[cfg(test)]
mod tests {
    use alloc::vec;
    use alloc::string::ToString;
    use crate::error::{ShellError, ErrorCategory};
    use crate::types::*;
    use crate::infrastructure::*;
    use crate::commands::CommandProcessor;

    #[test]
    fn test_shell_error_user_message() {
        let error = ShellError::InvalidCommand("nonexistent".to_string());
        assert_eq!(error.user_message(), "Command not found: nonexistent");
        
        let error = ShellError::FileNotFound("/path/to/file".to_string());
        assert_eq!(error.user_message(), "File not found: /path/to/file");
    }

    #[test]
    fn test_shell_error_category() {
        let error = ShellError::InvalidCommand("test".to_string());
        assert_eq!(error.category(), ErrorCategory::Parse);
        
        let error = ShellError::FileNotFound("test".to_string());
        assert_eq!(error.category(), ErrorCategory::FileSystem);
    }

    #[test]
    fn test_shell_error_suggestions() {
        let error = ShellError::InvalidCommand("ls".to_string());
        assert!(error.suggest_fix().is_some());
        
        let error = ShellError::InternalError("test".to_string());
        assert!(error.suggest_fix().is_none());
    }

    #[test]
    fn test_environment_variables() {
        let mut env = Environment::new();
        
        env.set_var("TEST_VAR".to_string(), "test_value".to_string());
        assert_eq!(env.get_var("TEST_VAR"), Some("test_value"));
        
        env.unset_var("TEST_VAR");
        assert_eq!(env.get_var("TEST_VAR"), None);
    }

    #[test]
    fn test_command_parser_basic() {
        let parser = CommandParser::new();
        
        let result = parser.parse("ls -la /home");
        assert!(result.is_ok());
        
        let parsed = result.unwrap();
        assert_eq!(parsed.command, "ls");
        assert_eq!(parsed.args, vec!["-la", "/home"]);
        assert!(!parsed.background);
        assert!(parsed.pipe_to.is_none());
    }

    #[test]
    fn test_command_parser_empty() {
        let parser = CommandParser::new();
        
        let result = parser.parse("");
        assert!(result.is_err());
        
        if let Err(ShellError::ParseError(_)) = result {
            // Expected error type
        } else {
            panic!("Expected ParseError");
        }
    }

    #[test]
    fn test_execution_context_initialization() {
        let mut context = ExecutionContext::new();
        
        let result = context.initialize();
        assert!(result.is_ok());
        
        // Check default environment variables
        assert_eq!(context.environment.get_var("PWD"), Some("/"));
        assert_eq!(context.environment.get_var("HOME"), Some("/home/user"));
        assert!(context.environment.get_var("PATH").is_some());
    }

    #[test]
    fn test_background_job_management() {
        let mut context = ExecutionContext::new();
        
        let job_id = context.add_background_job(123, "test command".to_string());
        assert_eq!(job_id, 1);
        
        let jobs = context.get_background_jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].pid, 123);
        assert_eq!(jobs[0].command, "test command");
        
        context.update_job_status(job_id, JobStatus::Completed(0));
        context.cleanup_completed_jobs();
        
        let jobs = context.get_background_jobs();
        assert_eq!(jobs.len(), 0);
    }

    #[test]
    fn test_command_processor_basic() {
        let mut processor = CommandProcessor::new();
        
        let result = processor.process_command("help");
        assert!(result.is_ok());
        
        let output = result.unwrap();
        assert!(output.contains("Available commands"));
    }

    #[test]
    fn test_command_processor_invalid_command() {
        let mut processor = CommandProcessor::new();
        
        let result = processor.process_command("nonexistent_command");
        assert!(result.is_err());
        
        if let Err(ShellError::InvalidCommand(cmd)) = result {
            assert_eq!(cmd, "nonexistent_command");
        } else {
            panic!("Expected InvalidCommand error");
        }
    }

    #[test]
    fn test_ls_flags_default() {
        let flags = LsFlags::default();
        assert!(!flags.long_format);
        assert!(!flags.show_hidden);
        assert!(!flags.human_readable);
        assert!(!flags.recursive);
    }
}