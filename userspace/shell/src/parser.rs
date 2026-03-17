#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::boxed::Box;
use crate::error::{ShellError, ShellResult};
use crate::types::{ParsedCommand, RedirectType, ConditionalType};

/// Token types produced by the tokenizer
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// A plain word or quoted string
    Word(String),
    /// Pipe operator |
    Pipe,
    /// Output redirect >
    RedirectOut,
    /// Append redirect >>
    RedirectAppend,
    /// Input redirect <
    RedirectIn,
    /// Error redirect 2>
    RedirectErr,
    /// And conditional &&
    And,
    /// Or conditional ||
    Or,
    /// Background operator &
    Background,
}

/// Advanced command parser with pipe, redirect, and conditional support
pub struct AdvancedParser;

impl AdvancedParser {
    pub fn new() -> Self {
        Self
    }

    /// Parse a full command line into a ParsedCommand chain.
    /// Supports pipes, redirects, conditionals, background, and quoted strings.
    pub fn parse(&self, input: &str) -> ShellResult<ParsedCommand> {
        let input = input.trim();
        if input.is_empty() {
            return Err(ShellError::ParseError("Empty command".to_string()));
        }

        let tokens = tokenize(input)?;
        if tokens.is_empty() {
            return Err(ShellError::ParseError("Empty command".to_string()));
        }

        parse_tokens(&tokens)
    }

    /// Parse with environment variable expansion.
    /// Expands $VAR and ${VAR} patterns using the provided lookup function.
    pub fn parse_with_env<F>(&self, input: &str, lookup: F) -> ShellResult<ParsedCommand>
    where
        F: Fn(&str) -> Option<String>,
    {
        let expanded = expand_variables(input, &lookup);
        self.parse(&expanded)
    }
}

/// Expand environment variables in the input string.
/// Supports $VAR and ${VAR} syntax. Single-quoted strings are not expanded.
pub fn expand_variables<F>(input: &str, lookup: &F) -> String
where
    F: Fn(&str) -> Option<String>,
{
    let mut result = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut in_single_quote = false;

    while i < len {
        let ch = chars[i];

        if ch == '\'' && !in_single_quote {
            in_single_quote = true;
            result.push(ch);
            i += 1;
            continue;
        }
        if ch == '\'' && in_single_quote {
            in_single_quote = false;
            result.push(ch);
            i += 1;
            continue;
        }
        if in_single_quote {
            result.push(ch);
            i += 1;
            continue;
        }

        if ch == '$' && i + 1 < len {
            i += 1;
            if chars[i] == '{' {
                // ${VAR} form
                i += 1;
                let start = i;
                while i < len && chars[i] != '}' {
                    i += 1;
                }
                let var_name: String = chars[start..i].iter().collect();
                if i < len {
                    i += 1; // skip '}'
                }
                if let Some(val) = lookup(&var_name) {
                    result.push_str(&val);
                }
            } else if chars[i] == '?' {
                // $? - last exit code, pass through for now
                if let Some(val) = lookup("?") {
                    result.push_str(&val);
                } else {
                    result.push_str("0");
                }
                i += 1;
            } else if is_var_start_char(chars[i]) {
                // $VAR form
                let start = i;
                while i < len && is_var_char(chars[i]) {
                    i += 1;
                }
                let var_name: String = chars[start..i].iter().collect();
                if let Some(val) = lookup(&var_name) {
                    result.push_str(&val);
                }
            } else {
                // Lone $ followed by non-variable char
                result.push('$');
                result.push(chars[i]);
                i += 1;
            }
        } else {
            result.push(ch);
            i += 1;
        }
    }

    result
}

fn is_var_start_char(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_var_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Tokenize a command line string into a sequence of tokens.
/// Handles single quotes, double quotes, and escape characters.
pub fn tokenize(input: &str) -> ShellResult<Vec<Token>> {
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < len {
        // Skip whitespace
        if chars[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Check for two-character operators first
        if i + 1 < len {
            let two = (chars[i], chars[i + 1]);
            match two {
                ('>', '>') => {
                    tokens.push(Token::RedirectAppend);
                    i += 2;
                    continue;
                }
                ('&', '&') => {
                    tokens.push(Token::And);
                    i += 2;
                    continue;
                }
                ('|', '|') => {
                    tokens.push(Token::Or);
                    i += 2;
                    continue;
                }
                ('2', '>') => {
                    // Only treat as redirect if preceded by whitespace or start of input
                    let prev_is_boundary = i == 0 || chars[i - 1].is_ascii_whitespace();
                    if prev_is_boundary {
                        tokens.push(Token::RedirectErr);
                        i += 2;
                        continue;
                    }
                }
                _ => {}
            }
        }

        // Single-character operators
        match chars[i] {
            '|' => {
                tokens.push(Token::Pipe);
                i += 1;
                continue;
            }
            '>' => {
                tokens.push(Token::RedirectOut);
                i += 1;
                continue;
            }
            '<' => {
                tokens.push(Token::RedirectIn);
                i += 1;
                continue;
            }
            '&' => {
                tokens.push(Token::Background);
                i += 1;
                continue;
            }
            _ => {}
        }

        // Word (possibly quoted)
        let (word, new_i) = read_word(&chars, i)?;
        tokens.push(Token::Word(word));
        i = new_i;
    }

    Ok(tokens)
}

/// Read a word token, handling single and double quotes and backslash escapes.
fn read_word(chars: &[char], start: usize) -> ShellResult<(String, usize)> {
    let len = chars.len();
    let mut word = String::new();
    let mut i = start;

    while i < len {
        let ch = chars[i];

        // Stop at whitespace or operator characters (unquoted)
        if ch.is_ascii_whitespace() || is_operator_char(ch, chars, i) {
            break;
        }

        if ch == '\'' {
            // Single-quoted string: no escaping inside
            i += 1;
            while i < len && chars[i] != '\'' {
                word.push(chars[i]);
                i += 1;
            }
            if i >= len {
                return Err(ShellError::ParseError(
                    "Unterminated single quote".to_string(),
                ));
            }
            i += 1; // skip closing quote
        } else if ch == '"' {
            // Double-quoted string: backslash escaping works
            i += 1;
            while i < len && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < len {
                    i += 1;
                    match chars[i] {
                        'n' => word.push('\n'),
                        't' => word.push('\t'),
                        '\\' => word.push('\\'),
                        '"' => word.push('"'),
                        '$' => word.push('$'),
                        other => {
                            word.push('\\');
                            word.push(other);
                        }
                    }
                } else {
                    word.push(chars[i]);
                }
                i += 1;
            }
            if i >= len {
                return Err(ShellError::ParseError(
                    "Unterminated double quote".to_string(),
                ));
            }
            i += 1; // skip closing quote
        } else if ch == '\\' && i + 1 < len {
            // Backslash escape outside quotes
            i += 1;
            word.push(chars[i]);
            i += 1;
        } else {
            word.push(ch);
            i += 1;
        }
    }

    Ok((word, i))
}

/// Check if the character at position `i` is the start of an operator.
fn is_operator_char(ch: char, chars: &[char], i: usize) -> bool {
    match ch {
        '|' | '>' | '<' => true,
        '&' => true,
        '2' => {
            // 2> is an operator only at a word boundary
            if i + 1 < chars.len() && chars[i + 1] == '>' {
                let prev_is_boundary = i == 0 || chars[i - 1].is_ascii_whitespace();
                prev_is_boundary
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Parse a flat list of tokens into a ParsedCommand chain.
fn parse_tokens(tokens: &[Token]) -> ShellResult<ParsedCommand> {
    // Split tokens by conditional operators (&&, ||) at the top level,
    // then by pipes, then handle redirects within each simple command.
    parse_conditional(tokens)
}

/// Parse conditional chains (&&, ||). These have the lowest precedence.
fn parse_conditional(tokens: &[Token]) -> ShellResult<ParsedCommand> {
    // Find the LAST conditional operator to create right-associative chaining
    // Actually, shell conditionals are left-associative, so find the FIRST one.
    // But we model it as: the left side is a single pipeline, the right side
    // is the rest (which may contain more conditionals).
    
    // Find the first && or || at the top level
    for (i, token) in tokens.iter().enumerate() {
        match token {
            Token::And | Token::Or => {
                if i == 0 {
                    return Err(ShellError::ParseError(
                        "Unexpected conditional operator at start".to_string(),
                    ));
                }
                let left_tokens = &tokens[..i];
                let right_tokens = &tokens[i + 1..];
                if right_tokens.is_empty() {
                    return Err(ShellError::ParseError(
                        "Expected command after conditional operator".to_string(),
                    ));
                }

                let cond_type = match token {
                    Token::And => ConditionalType::And,
                    Token::Or => ConditionalType::Or,
                    _ => unreachable!(),
                };

                // Parse left side as a pipeline
                let mut left = parse_pipeline(left_tokens)?;
                // Parse right side which may contain more conditionals
                let right = parse_conditional(right_tokens)?;

                // Attach the conditional to the leftmost command's last pipe segment
                // We need to find the tail of the pipe chain and set its conditional
                set_tail_conditional(&mut left, cond_type, right);

                return Ok(left);
            }
            _ => {}
        }
    }

    // No conditional found, parse as pipeline
    parse_pipeline(tokens)
}

/// Set the conditional on the tail (last pipe segment) of a command chain,
/// and attach the next command as a new ParsedCommand linked via conditional.
fn set_tail_conditional(
    cmd: &mut ParsedCommand,
    cond: ConditionalType,
    next: ParsedCommand,
) {
    // Walk to the end of the pipe chain
    let tail = get_pipe_tail(cmd);
    tail.conditional = Some(cond);
    // We store the next command in pipe_to of a wrapper, but that conflates
    // pipe and conditional. Instead, let's use a different approach:
    // The conditional's "next" command is stored as a separate field.
    // Looking at the ParsedCommand struct, conditional is just a type marker.
    // The actual next command needs to be stored somewhere.
    // 
    // The design has: conditional: Option<ConditionalType> on ParsedCommand.
    // This means the conditional type indicates what connects THIS command
    // to the NEXT command in the chain. We need to store the next command.
    //
    // Since ParsedCommand has pipe_to for pipes, we'll use a convention:
    // For conditionals, we create a wrapper. But the struct doesn't have
    // a "next" field for conditionals.
    //
    // Let's re-examine: The design shows pipe_to as Option<Box<ParsedCommand>>.
    // We can repurpose this: if conditional is Some, then pipe_to holds the
    // conditional-next command. If conditional is None and pipe_to is Some,
    // it's a pipe.
    //
    // Actually, this won't work cleanly for "a | b && c | d".
    // Let's keep it simple: conditional and pipe_to are independent.
    // pipe_to = piped command, conditional = what connects to next_command.
    // But there's no next_command field...
    //
    // For now, we'll store the conditional next command in a flat chain:
    // The tail of the pipe chain gets conditional set, and the next command
    // is stored in the tail's pipe_to (overloading pipe_to for both purposes).
    // The caller can distinguish by checking if conditional is Some.
    tail.pipe_to = Some(Box::new(next));
}

/// Get a mutable reference to the last command in a pipe chain.
fn get_pipe_tail(cmd: &mut ParsedCommand) -> &mut ParsedCommand {
    // If this command has a pipe_to and no conditional set, follow the chain
    if cmd.pipe_to.is_some() && cmd.conditional.is_none() {
        get_pipe_tail(cmd.pipe_to.as_mut().unwrap())
    } else {
        cmd
    }
}

/// Parse a pipeline (commands separated by |).
fn parse_pipeline(tokens: &[Token]) -> ShellResult<ParsedCommand> {
    // Split by Pipe tokens
    let mut segments: Vec<&[Token]> = Vec::new();
    let mut start = 0;

    for (i, token) in tokens.iter().enumerate() {
        if *token == Token::Pipe {
            if i == start {
                return Err(ShellError::ParseError(
                    "Unexpected pipe operator".to_string(),
                ));
            }
            segments.push(&tokens[start..i]);
            start = i + 1;
        }
    }
    // Last segment
    if start >= tokens.len() {
        return Err(ShellError::ParseError(
            "Expected command after pipe".to_string(),
        ));
    }
    segments.push(&tokens[start..]);

    // Parse each segment as a simple command, then chain them
    let mut commands: Vec<ParsedCommand> = Vec::new();
    for seg in &segments {
        commands.push(parse_simple_command(seg)?);
    }

    // Chain commands via pipe_to (right to left)
    let mut result = commands.pop().unwrap();
    while let Some(mut cmd) = commands.pop() {
        cmd.pipe_to = Some(Box::new(result));
        result = cmd;
    }

    Ok(result)
}

/// Parse a simple command (no pipes or conditionals, but may have redirects and background).
fn parse_simple_command(tokens: &[Token]) -> ShellResult<ParsedCommand> {
    let mut command = String::new();
    let mut args: Vec<String> = Vec::new();
    let mut input_redirect: Option<String> = None;
    let mut output_redirect: Option<RedirectType> = None;
    let mut background = false;

    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Word(w) => {
                if command.is_empty() {
                    command = w.clone();
                } else {
                    args.push(w.clone());
                }
                i += 1;
            }
            Token::RedirectOut => {
                i += 1;
                if i >= tokens.len() {
                    return Err(ShellError::ParseError(
                        "Expected filename after >".to_string(),
                    ));
                }
                if let Token::Word(filename) = &tokens[i] {
                    output_redirect = Some(RedirectType::Overwrite(filename.clone()));
                    i += 1;
                } else {
                    return Err(ShellError::ParseError(
                        "Expected filename after >".to_string(),
                    ));
                }
            }
            Token::RedirectAppend => {
                i += 1;
                if i >= tokens.len() {
                    return Err(ShellError::ParseError(
                        "Expected filename after >>".to_string(),
                    ));
                }
                if let Token::Word(filename) = &tokens[i] {
                    output_redirect = Some(RedirectType::Append(filename.clone()));
                    i += 1;
                } else {
                    return Err(ShellError::ParseError(
                        "Expected filename after >>".to_string(),
                    ));
                }
            }
            Token::RedirectIn => {
                i += 1;
                if i >= tokens.len() {
                    return Err(ShellError::ParseError(
                        "Expected filename after <".to_string(),
                    ));
                }
                if let Token::Word(filename) = &tokens[i] {
                    input_redirect = Some(filename.clone());
                    i += 1;
                } else {
                    return Err(ShellError::ParseError(
                        "Expected filename after <".to_string(),
                    ));
                }
            }
            Token::RedirectErr => {
                i += 1;
                if i >= tokens.len() {
                    return Err(ShellError::ParseError(
                        "Expected filename after 2>".to_string(),
                    ));
                }
                if let Token::Word(filename) = &tokens[i] {
                    output_redirect = Some(RedirectType::Error(filename.clone()));
                    i += 1;
                } else {
                    return Err(ShellError::ParseError(
                        "Expected filename after 2>".to_string(),
                    ));
                }
            }
            Token::Background => {
                background = true;
                i += 1;
                // Background should typically be at the end
            }
            Token::Pipe | Token::And | Token::Or => {
                // These should have been handled at a higher level
                return Err(ShellError::ParseError(
                    "Unexpected operator in simple command".to_string(),
                ));
            }
        }
    }

    if command.is_empty() {
        return Err(ShellError::ParseError("Empty command".to_string()));
    }

    Ok(ParsedCommand {
        command,
        args,
        input_redirect,
        output_redirect,
        pipe_to: None,
        background,
        conditional: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    // ==================== Tokenizer Tests ====================

    #[test]
    fn test_tokenize_simple_command() {
        let tokens = tokenize("ls -la /home").unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0], Token::Word("ls".to_string()));
        assert_eq!(tokens[1], Token::Word("-la".to_string()));
        assert_eq!(tokens[2], Token::Word("/home".to_string()));
    }

    #[test]
    fn test_tokenize_pipe() {
        let tokens = tokenize("ls | grep foo").unwrap();
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0], Token::Word("ls".to_string()));
        assert_eq!(tokens[1], Token::Pipe);
        assert_eq!(tokens[2], Token::Word("grep".to_string()));
        assert_eq!(tokens[3], Token::Word("foo".to_string()));
    }

    #[test]
    fn test_tokenize_redirect_out() {
        let tokens = tokenize("echo hello > file.txt").unwrap();
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0], Token::Word("echo".to_string()));
        assert_eq!(tokens[1], Token::Word("hello".to_string()));
        assert_eq!(tokens[2], Token::RedirectOut);
        assert_eq!(tokens[3], Token::Word("file.txt".to_string()));
    }

    #[test]
    fn test_tokenize_redirect_append() {
        let tokens = tokenize("echo hello >> file.txt").unwrap();
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[2], Token::RedirectAppend);
    }

    #[test]
    fn test_tokenize_redirect_in() {
        let tokens = tokenize("cat < input.txt").unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0], Token::Word("cat".to_string()));
        assert_eq!(tokens[1], Token::RedirectIn);
        assert_eq!(tokens[2], Token::Word("input.txt".to_string()));
    }

    #[test]
    fn test_tokenize_redirect_err() {
        let tokens = tokenize("cmd 2> error.log").unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0], Token::Word("cmd".to_string()));
        assert_eq!(tokens[1], Token::RedirectErr);
        assert_eq!(tokens[2], Token::Word("error.log".to_string()));
    }

    #[test]
    fn test_tokenize_and_conditional() {
        let tokens = tokenize("cmd1 && cmd2").unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0], Token::Word("cmd1".to_string()));
        assert_eq!(tokens[1], Token::And);
        assert_eq!(tokens[2], Token::Word("cmd2".to_string()));
    }

    #[test]
    fn test_tokenize_or_conditional() {
        let tokens = tokenize("cmd1 || cmd2").unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0], Token::Word("cmd1".to_string()));
        assert_eq!(tokens[1], Token::Or);
        assert_eq!(tokens[2], Token::Word("cmd2".to_string()));
    }

    #[test]
    fn test_tokenize_background() {
        let tokens = tokenize("sleep 10 &").unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0], Token::Word("sleep".to_string()));
        assert_eq!(tokens[1], Token::Word("10".to_string()));
        assert_eq!(tokens[2], Token::Background);
    }

    #[test]
    fn test_tokenize_single_quotes() {
        let tokens = tokenize("echo 'hello world'").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], Token::Word("echo".to_string()));
        assert_eq!(tokens[1], Token::Word("hello world".to_string()));
    }

    #[test]
    fn test_tokenize_double_quotes() {
        let tokens = tokenize("echo \"hello world\"").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], Token::Word("echo".to_string()));
        assert_eq!(tokens[1], Token::Word("hello world".to_string()));
    }

    #[test]
    fn test_tokenize_escape_in_double_quotes() {
        let tokens = tokenize("echo \"hello\\nworld\"").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[1], Token::Word("hello\nworld".to_string()));
    }

    #[test]
    fn test_tokenize_backslash_escape() {
        let tokens = tokenize("echo hello\\ world").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[1], Token::Word("hello world".to_string()));
    }

    #[test]
    fn test_tokenize_unterminated_single_quote() {
        let result = tokenize("echo 'hello");
        assert!(result.is_err());
    }

    #[test]
    fn test_tokenize_unterminated_double_quote() {
        let result = tokenize("echo \"hello");
        assert!(result.is_err());
    }

    // ==================== Variable Expansion Tests ====================

    #[test]
    fn test_expand_simple_var() {
        let lookup = |name: &str| -> Option<String> {
            match name {
                "HOME" => Some("/home/user".to_string()),
                _ => None,
            }
        };
        let result = expand_variables("cd $HOME", &lookup);
        assert_eq!(result, "cd /home/user");
    }

    #[test]
    fn test_expand_braced_var() {
        let lookup = |name: &str| -> Option<String> {
            match name {
                "PATH" => Some("/bin:/usr/bin".to_string()),
                _ => None,
            }
        };
        let result = expand_variables("echo ${PATH}", &lookup);
        assert_eq!(result, "echo /bin:/usr/bin");
    }

    #[test]
    fn test_expand_undefined_var() {
        let lookup = |_: &str| -> Option<String> { None };
        let result = expand_variables("echo $UNDEFINED", &lookup);
        assert_eq!(result, "echo ");
    }

    #[test]
    fn test_expand_no_expansion_in_single_quotes() {
        let lookup = |name: &str| -> Option<String> {
            match name {
                "VAR" => Some("value".to_string()),
                _ => None,
            }
        };
        let result = expand_variables("echo '$VAR'", &lookup);
        assert_eq!(result, "echo '$VAR'");
    }

    #[test]
    fn test_expand_multiple_vars() {
        let lookup = |name: &str| -> Option<String> {
            match name {
                "A" => Some("1".to_string()),
                "B" => Some("2".to_string()),
                _ => None,
            }
        };
        let result = expand_variables("echo $A and $B", &lookup);
        assert_eq!(result, "echo 1 and 2");
    }

    // ==================== Parser Tests ====================

    #[test]
    fn test_parse_simple_command() {
        let parser = AdvancedParser::new();
        let cmd = parser.parse("ls -la /home").unwrap();
        assert_eq!(cmd.command, "ls");
        assert_eq!(cmd.args, vec!["-la", "/home"]);
        assert!(!cmd.background);
        assert!(cmd.pipe_to.is_none());
        assert!(cmd.conditional.is_none());
    }

    #[test]
    fn test_parse_empty_command() {
        let parser = AdvancedParser::new();
        let result = parser.parse("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_whitespace_only() {
        let parser = AdvancedParser::new();
        let result = parser.parse("   ");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_pipe() {
        let parser = AdvancedParser::new();
        let cmd = parser.parse("ls | grep foo").unwrap();
        assert_eq!(cmd.command, "ls");
        assert!(cmd.pipe_to.is_some());
        let piped = cmd.pipe_to.as_ref().unwrap();
        assert_eq!(piped.command, "grep");
        assert_eq!(piped.args, vec!["foo"]);
    }

    #[test]
    fn test_parse_multiple_pipes() {
        let parser = AdvancedParser::new();
        let cmd = parser.parse("cat file | grep pattern | wc -l").unwrap();
        assert_eq!(cmd.command, "cat");
        let p1 = cmd.pipe_to.as_ref().unwrap();
        assert_eq!(p1.command, "grep");
        let p2 = p1.pipe_to.as_ref().unwrap();
        assert_eq!(p2.command, "wc");
        assert_eq!(p2.args, vec!["-l"]);
    }

    #[test]
    fn test_parse_output_redirect() {
        let parser = AdvancedParser::new();
        let cmd = parser.parse("echo hello > output.txt").unwrap();
        assert_eq!(cmd.command, "echo");
        assert_eq!(cmd.args, vec!["hello"]);
        match &cmd.output_redirect {
            Some(RedirectType::Overwrite(f)) => assert_eq!(f, "output.txt"),
            _ => panic!("Expected Overwrite redirect"),
        }
    }

    #[test]
    fn test_parse_append_redirect() {
        let parser = AdvancedParser::new();
        let cmd = parser.parse("echo hello >> output.txt").unwrap();
        match &cmd.output_redirect {
            Some(RedirectType::Append(f)) => assert_eq!(f, "output.txt"),
            _ => panic!("Expected Append redirect"),
        }
    }

    #[test]
    fn test_parse_input_redirect() {
        let parser = AdvancedParser::new();
        let cmd = parser.parse("cat < input.txt").unwrap();
        assert_eq!(cmd.command, "cat");
        assert_eq!(cmd.input_redirect, Some("input.txt".to_string()));
    }

    #[test]
    fn test_parse_error_redirect() {
        let parser = AdvancedParser::new();
        let cmd = parser.parse("cmd 2> error.log").unwrap();
        match &cmd.output_redirect {
            Some(RedirectType::Error(f)) => assert_eq!(f, "error.log"),
            _ => panic!("Expected Error redirect"),
        }
    }

    #[test]
    fn test_parse_background() {
        let parser = AdvancedParser::new();
        let cmd = parser.parse("sleep 10 &").unwrap();
        assert_eq!(cmd.command, "sleep");
        assert!(cmd.background);
    }

    #[test]
    fn test_parse_and_conditional() {
        let parser = AdvancedParser::new();
        let cmd = parser.parse("cmd1 && cmd2").unwrap();
        assert_eq!(cmd.command, "cmd1");
        assert_eq!(cmd.conditional, Some(ConditionalType::And));
        let next = cmd.pipe_to.as_ref().unwrap();
        assert_eq!(next.command, "cmd2");
    }

    #[test]
    fn test_parse_or_conditional() {
        let parser = AdvancedParser::new();
        let cmd = parser.parse("cmd1 || cmd2").unwrap();
        assert_eq!(cmd.command, "cmd1");
        assert_eq!(cmd.conditional, Some(ConditionalType::Or));
        let next = cmd.pipe_to.as_ref().unwrap();
        assert_eq!(next.command, "cmd2");
    }

    #[test]
    fn test_parse_quoted_args() {
        let parser = AdvancedParser::new();
        let cmd = parser.parse("echo 'hello world' \"foo bar\"").unwrap();
        assert_eq!(cmd.command, "echo");
        assert_eq!(cmd.args, vec!["hello world", "foo bar"]);
    }

    #[test]
    fn test_parse_complex_command() {
        let parser = AdvancedParser::new();
        let cmd = parser.parse("cat file.txt | grep 'pattern' > output.txt").unwrap();
        assert_eq!(cmd.command, "cat");
        let piped = cmd.pipe_to.as_ref().unwrap();
        assert_eq!(piped.command, "grep");
        assert_eq!(piped.args, vec!["pattern"]);
        match &piped.output_redirect {
            Some(RedirectType::Overwrite(f)) => assert_eq!(f, "output.txt"),
            _ => panic!("Expected redirect on piped command"),
        }
    }

    #[test]
    fn test_parse_pipe_and_conditional() {
        let parser = AdvancedParser::new();
        let cmd = parser.parse("ls | grep foo && echo found").unwrap();
        assert_eq!(cmd.command, "ls");
        let piped = cmd.pipe_to.as_ref().unwrap();
        assert_eq!(piped.command, "grep");
        assert_eq!(piped.conditional, Some(ConditionalType::And));
        let next = piped.pipe_to.as_ref().unwrap();
        assert_eq!(next.command, "echo");
    }

    #[test]
    fn test_parse_missing_redirect_target() {
        let parser = AdvancedParser::new();
        let result = parser.parse("echo hello >");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_missing_pipe_command() {
        let parser = AdvancedParser::new();
        let result = parser.parse("ls |");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_with_env() {
        let parser = AdvancedParser::new();
        let lookup = |name: &str| -> Option<String> {
            match name {
                "DIR" => Some("/home".to_string()),
                _ => None,
            }
        };
        let cmd = parser.parse_with_env("ls $DIR", lookup).unwrap();
        assert_eq!(cmd.command, "ls");
        assert_eq!(cmd.args, vec!["/home"]);
    }
}
