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
    fn test_environment_with_defaults() {
        let env = Environment::with_defaults();
        
        assert_eq!(env.get_var("PWD"), Some("/"));
        assert_eq!(env.get_var("HOME"), Some("/home/user"));
        assert_eq!(env.get_var("PATH"), Some("/bin:/usr/bin"));
        assert_eq!(env.get_var("SHELL"), Some("/bin/kosh-shell"));
        assert_eq!(env.get_var("USER"), Some("user"));
        assert_eq!(env.get_var("HOSTNAME"), Some("kosh"));
    }

    #[test]
    fn test_environment_expand_variables() {
        let mut env = Environment::new();
        env.set_var("HOME".to_string(), "/home/user".to_string());
        env.set_var("NAME".to_string(), "test".to_string());
        
        // Test $VAR expansion
        assert_eq!(env.expand_variables("cd $HOME"), "cd /home/user");
        
        // Test ${VAR} expansion
        assert_eq!(env.expand_variables("echo ${NAME}"), "echo test");
        
        // Test multiple variables
        assert_eq!(env.expand_variables("$HOME/$NAME"), "/home/user/test");
        
        // Test undefined variable (expands to empty)
        assert_eq!(env.expand_variables("$UNDEFINED"), "");
        
        // Test single quotes (no expansion)
        assert_eq!(env.expand_variables("'$HOME'"), "'$HOME'");
    }

    #[test]
    fn test_environment_path_entries() {
        let mut env = Environment::new();
        env.set_var("PATH".to_string(), "/bin:/usr/bin:/usr/local/bin".to_string());
        
        let entries = env.path_entries();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0], "/bin");
        assert_eq!(entries[1], "/usr/bin");
        assert_eq!(entries[2], "/usr/local/bin");
    }

    #[test]
    fn test_environment_format_all() {
        let mut env = Environment::new();
        env.set_var("A".to_string(), "1".to_string());
        env.set_var("B".to_string(), "2".to_string());
        
        let output = env.format_all();
        // BTreeMap is sorted, so A comes before B
        assert!(output.contains("A=1"));
        assert!(output.contains("B=2"));
    }

    #[test]
    fn test_environment_parse_assignment() {
        // Valid assignments
        let result = Environment::parse_assignment("NAME=value");
        assert_eq!(result, Some(("NAME".to_string(), "value".to_string())));
        
        let result = Environment::parse_assignment("_VAR=test");
        assert_eq!(result, Some(("_VAR".to_string(), "test".to_string())));
        
        let result = Environment::parse_assignment("VAR123=abc");
        assert_eq!(result, Some(("VAR123".to_string(), "abc".to_string())));
        
        // Empty value is valid
        let result = Environment::parse_assignment("EMPTY=");
        assert_eq!(result, Some(("EMPTY".to_string(), "".to_string())));
        
        // Invalid: no equals sign
        let result = Environment::parse_assignment("NOEQUALS");
        assert_eq!(result, None);
        
        // Invalid: empty name
        let result = Environment::parse_assignment("=value");
        assert_eq!(result, None);
        
        // Invalid: name starts with digit
        let result = Environment::parse_assignment("1VAR=value");
        assert_eq!(result, None);
    }

    #[test]
    fn test_environment_is_valid_var_name() {
        assert!(Environment::is_valid_var_name("VAR"));
        assert!(Environment::is_valid_var_name("_VAR"));
        assert!(Environment::is_valid_var_name("VAR123"));
        assert!(Environment::is_valid_var_name("_123"));
        assert!(Environment::is_valid_var_name("a"));
        
        assert!(!Environment::is_valid_var_name(""));
        assert!(!Environment::is_valid_var_name("123VAR"));
        assert!(!Environment::is_valid_var_name("VAR-NAME"));
        assert!(!Environment::is_valid_var_name("VAR.NAME"));
    }

    #[test]
    fn test_environment_pwd_sync() {
        let mut env = Environment::new();
        env.set_var("PWD".to_string(), "/home/user".to_string());
        
        // working_directory should be updated when PWD is set
        assert_eq!(env.working_directory, "/home/user");
    }

    #[test]
    fn test_cmd_export_set_variable() {
        let mut env = Environment::new();
        
        let result = CommandProcessor::process_env_command("export", &["TEST=value"], &mut env);
        assert!(result.is_some());
        assert!(result.unwrap().is_ok());
        assert_eq!(env.get_var("TEST"), Some("value"));
    }

    #[test]
    fn test_cmd_export_list_variables() {
        let mut env = Environment::new();
        env.set_var("VAR1".to_string(), "val1".to_string());
        env.set_var("VAR2".to_string(), "val2".to_string());
        
        let result = CommandProcessor::process_env_command("export", &[], &mut env);
        assert!(result.is_some());
        let output = result.unwrap().unwrap();
        assert!(output.contains("export VAR1=\"val1\""));
        assert!(output.contains("export VAR2=\"val2\""));
    }

    #[test]
    fn test_cmd_export_invalid_name() {
        let mut env = Environment::new();
        
        let result = CommandProcessor::process_env_command("export", &["123INVALID=value"], &mut env);
        assert!(result.is_some());
        assert!(result.unwrap().is_err());
    }

    #[test]
    fn test_cmd_unset_variable() {
        let mut env = Environment::new();
        env.set_var("TEST".to_string(), "value".to_string());
        
        let result = CommandProcessor::process_env_command("unset", &["TEST"], &mut env);
        assert!(result.is_some());
        assert!(result.unwrap().is_ok());
        assert_eq!(env.get_var("TEST"), None);
    }

    #[test]
    fn test_cmd_unset_no_args() {
        let mut env = Environment::new();
        
        let result = CommandProcessor::process_env_command("unset", &[], &mut env);
        assert!(result.is_some());
        assert!(result.unwrap().is_err());
    }

    #[test]
    fn test_cmd_env_list() {
        let mut env = Environment::new();
        env.set_var("A".to_string(), "1".to_string());
        env.set_var("B".to_string(), "2".to_string());
        
        let result = CommandProcessor::process_env_command("env", &[], &mut env);
        assert!(result.is_some());
        let output = result.unwrap().unwrap();
        assert!(output.contains("A=1"));
        assert!(output.contains("B=2"));
    }

    #[test]
    fn test_cmd_env_not_env_command() {
        let mut env = Environment::new();
        
        // Non-env commands should return None
        let result = CommandProcessor::process_env_command("ls", &[], &mut env);
        assert!(result.is_none());
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
        assert!(output.contains("export"));
        assert!(output.contains("unset"));
        assert!(output.contains("env"));
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

    // ── Service Client Tests ──────────────────────────────────────────

    use kosh_service::ServiceType;

    #[test]
    fn test_service_endpoint_new() {
        let endpoint = ServiceEndpoint::new(ServiceType::FileSystem, "fs_service");
        assert_eq!(endpoint.name, "fs_service");
        assert_eq!(endpoint.service_type, ServiceType::FileSystem);
        assert_eq!(endpoint.status, ServiceConnectionStatus::Disconnected);
        assert!(endpoint.pid.is_none());
        assert!(!endpoint.is_available());
    }

    #[test]
    fn test_service_endpoint_connect() {
        let mut endpoint = ServiceEndpoint::new(ServiceType::FileSystem, "fs_service");
        endpoint.connect(100);
        
        assert_eq!(endpoint.pid, Some(100));
        assert_eq!(endpoint.status, ServiceConnectionStatus::Connected);
        assert!(endpoint.is_available());
        assert_eq!(endpoint.retry_count, 0);
        assert_eq!(endpoint.consecutive_failures, 0);
    }

    #[test]
    fn test_service_endpoint_disconnect() {
        let mut endpoint = ServiceEndpoint::new(ServiceType::FileSystem, "fs_service");
        endpoint.connect(100);
        endpoint.disconnect();
        
        assert!(endpoint.pid.is_none());
        assert_eq!(endpoint.status, ServiceConnectionStatus::Disconnected);
        assert!(!endpoint.is_available());
    }

    #[test]
    fn test_service_endpoint_record_success() {
        let mut endpoint = ServiceEndpoint::new(ServiceType::FileSystem, "fs_service");
        endpoint.connect(100);
        endpoint.consecutive_failures = 2;
        
        endpoint.record_success();
        
        assert_eq!(endpoint.consecutive_failures, 0);
        assert_eq!(endpoint.status, ServiceConnectionStatus::Connected);
    }

    #[test]
    fn test_service_endpoint_record_failure() {
        let mut endpoint = ServiceEndpoint::new(ServiceType::FileSystem, "fs_service");
        endpoint.connect(100);
        
        // First two failures should not trigger reconnection
        assert!(!endpoint.record_failure());
        assert_eq!(endpoint.consecutive_failures, 1);
        assert!(!endpoint.record_failure());
        assert_eq!(endpoint.consecutive_failures, 2);
        
        // Third failure should mark as unreachable and suggest reconnection
        assert!(endpoint.record_failure());
        assert_eq!(endpoint.consecutive_failures, 3);
        assert_eq!(endpoint.status, ServiceConnectionStatus::Unreachable);
    }

    #[test]
    fn test_service_client_new() {
        let client = ShellServiceClient::new();
        assert!(!client.all_services_available());
        assert!(client.fs_service_pid().is_none());
        assert!(client.process_service_pid().is_none());
        assert!(client.driver_service_pid().is_none());
    }

    #[test]
    fn test_service_client_discover_services() {
        let mut client = ShellServiceClient::new();
        let result = client.discover_services();
        assert!(result.is_ok());
        
        assert!(client.all_services_available());
        assert_eq!(client.fs_service_pid(), Some(100));
        assert_eq!(client.process_service_pid(), Some(101));
        assert_eq!(client.driver_service_pid(), Some(102));
    }

    #[test]
    fn test_service_client_find_service_by_name() {
        let mut client = ShellServiceClient::new();
        client.discover_services().unwrap();
        
        assert_eq!(client.find_service_by_name("fs_service"), Some(100));
        assert_eq!(client.find_service_by_name("process_service"), Some(101));
        assert_eq!(client.find_service_by_name("driver_service"), Some(102));
        assert_eq!(client.find_service_by_name("nonexistent"), None);
    }

    #[test]
    fn test_service_client_get_service_status() {
        let mut client = ShellServiceClient::new();
        
        // Before discovery, all should be disconnected
        assert_eq!(
            client.get_service_status(ServiceType::FileSystem),
            ServiceConnectionStatus::Disconnected
        );
        
        client.discover_services().unwrap();
        
        assert_eq!(
            client.get_service_status(ServiceType::FileSystem),
            ServiceConnectionStatus::Connected
        );
        assert_eq!(
            client.get_service_status(ServiceType::ProcessManager),
            ServiceConnectionStatus::Connected
        );
        assert_eq!(
            client.get_service_status(ServiceType::DriverManager),
            ServiceConnectionStatus::Connected
        );
    }

    #[test]
    fn test_service_client_get_service_pid() {
        let mut client = ShellServiceClient::new();
        assert!(client.get_service_pid(ServiceType::FileSystem).is_none());
        
        client.discover_services().unwrap();
        assert_eq!(client.get_service_pid(ServiceType::FileSystem), Some(100));
    }

    #[test]
    fn test_service_client_send_fs_request() {
        let mut client = ShellServiceClient::new();
        client.discover_services().unwrap();
        
        let request = FileSystemRequest::List { path: "/".to_string() };
        let result = client.send_fs_request(request);
        assert!(result.is_ok());
        
        let response = result.unwrap();
        assert_eq!(response.status, ServiceResponseStatus::Success);
    }

    #[test]
    fn test_service_client_send_process_request() {
        let mut client = ShellServiceClient::new();
        client.discover_services().unwrap();
        
        let request = ProcessRequest::List;
        let result = client.send_process_request(request);
        assert!(result.is_ok());
    }

    #[test]
    fn test_service_client_send_driver_request() {
        let mut client = ShellServiceClient::new();
        client.discover_services().unwrap();
        
        let request = DriverRequest::List;
        let result = client.send_driver_request(request);
        assert!(result.is_ok());
    }

    #[test]
    fn test_service_client_request_without_discovery() {
        let mut client = ShellServiceClient::new();
        
        // Without discovery, requests should trigger reconnection (which succeeds
        // in the simulated environment), so the request should still succeed.
        let request = FileSystemRequest::List { path: "/".to_string() };
        let result = client.send_fs_request(request);
        // Reconnection is simulated to succeed, so this should work
        assert!(result.is_ok());
    }

    #[test]
    fn test_service_client_set_timeout() {
        let mut client = ShellServiceClient::new();
        client.set_timeout(2000);
        // Timeout is internal state; just verify it doesn't panic
    }

    #[test]
    fn test_service_client_tick() {
        let mut client = ShellServiceClient::new();
        client.tick();
        client.tick();
        // Tick advances internal counter; verify no panic
    }

    #[test]
    fn test_service_client_reconnect_all() {
        let mut client = ShellServiceClient::new();
        // All services start disconnected, reconnect_all should connect them
        let reconnected = client.reconnect_all();
        assert_eq!(reconnected, 3);
        assert!(client.all_services_available());
    }

    #[test]
    fn test_service_client_health_check() {
        let mut client = ShellServiceClient::new();
        client.discover_services().unwrap();
        
        // Health check should not panic and services should remain connected
        client.check_service_health();
        assert!(client.all_services_available());
    }

    #[test]
    fn test_service_endpoint_retry_limit() {
        let mut endpoint = ServiceEndpoint::new(ServiceType::FileSystem, "fs_service");
        endpoint.connect(100);
        
        // Trigger 3 consecutive failures to mark as unreachable
        endpoint.record_failure();
        endpoint.record_failure();
        let should_reconnect = endpoint.record_failure();
        assert!(should_reconnect);
        assert_eq!(endpoint.status, ServiceConnectionStatus::Unreachable);
        
        // Simulate max retries exhausted
        endpoint.retry_count = 3; // MAX_RETRY_ATTEMPTS
        let should_reconnect = endpoint.record_failure();
        // After max retries, should not suggest reconnection
        assert!(!should_reconnect);
    }
}