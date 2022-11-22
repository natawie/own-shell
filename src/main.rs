#![warn(clippy::unwrap_used)]

use std::io::Stdin;
use std::io::{stdout, stdin, Write};
use std::ffi::CString;
use std::process::exit;
use std::env;
use std::fs;
use std::path::Path;

use nix::unistd::{
    ForkResult::{Child, Parent},
    fork,
    execvp,
    chdir,
    getcwd,
};

use nix::sys::wait::wait;
use nix::sys::wait::WaitStatus;

#[derive(Debug)]
struct Shell {
    last_exit_status: i32,
    // idk how to implement ! better
    flip_exit_status: bool,
    skip_next: bool,
    stdin: Stdin,
}

#[derive(Debug, Clone, PartialEq)]
enum Token<'t> {
    // // shell-defined commands
    // Cd,
    // Echo,
    // Exit,
    // False,
    // LastExit,
    // Pwd,
    // True,
    // operators
    EndLine,
    IfFalse,
    IfTrue,
    Negate,
    OpenBrace,
    CloseBrace,
    // external commands
    Command(&'t str),
    // arguments
    Argument(CString),
}

impl Shell {
    fn new() -> Self {
        Self {
            last_exit_status: 0,
            flip_exit_status: false,
            skip_next: false,
            stdin: stdin()
        }
    }

    fn set_exit_status(&mut self, status: i32) {
        if self.flip_exit_status {
            if status != 0 {
                self.last_exit_status = 0;
            } else {
                self.last_exit_status = 1;
            }
            self.flip_exit_status = false;
        } else {
            self.last_exit_status = status;
        }
    }

    fn execute_process(&mut self, process: &str, args: Vec<CString>) {
        match process {
            "||" => {
                if self.last_exit_status == 0 {
                    self.skip_next = true;
                }
            },
            "cd" => {
                // TODO: empty cd goes to $HOME
                if args.len() == 1 {
                    println!("TODO: empty cd goes to $HOME");
                    return
                }
                if args.len() > 2 {
                    self.set_exit_status(1);
                    return
                }
                if let Some(v) = args.get(1) {
                    chdir(v.as_c_str()).unwrap();
                    self.set_exit_status(0);
                } else {
                    self.last_exit_status = 1;
                    self.set_exit_status(1);
                }
            },
            "echo" => {
                let args: Vec<String> = args.into_iter().map(|x| x.into_string().unwrap()).collect();
                println!("{}", args[1..].join(" "));
                exit(0);
            }
            "pwd" => {
                println!("{}", getcwd().unwrap().to_str().unwrap()); // this is very safe
                exit(0);
            },
            "lastexit" => {
                println!("{}", self.last_exit_status);
                exit(0);
            },
            "exec" => {
                self.execute_process(args[1].clone().to_str().unwrap(), args);
            },
            "true" => {
                exit(0)
            },
            "false" => {
                exit(1)
            },
            _ => {
                let mut process_path = String::new();
                if !process.starts_with("/") {
                    let paths = env::var("PATH").unwrap_or(String::new());
                    let paths: Vec<&str> = paths.split(':').collect();
                    for path in paths {
                        if !process_path.is_empty() {
                            break
                        }
                        if !Path::new(path).exists() {
                            continue
                        }
                        let results = fs::read_dir(path).unwrap();
                        for result in results {
                            let result = result.unwrap();
                            if result.path().ends_with(&process) {
                                process_path = result.path().to_str().unwrap().to_string();
                                break
                            }
                        }
                    }
                } else {
                    process_path = process.to_string();
                }
                println!("{} {:?}", process_path, args);

                match execvp(CString::new(process_path).unwrap().as_c_str(), &args) {
                    Ok(_) => {},
                    Err(e) => {
                        println!("{:?}", e)
                    }
                };
            }
        }
    }

    fn run(&mut self) {
        print!("{} $ ", getcwd().unwrap().to_str().unwrap());
        stdout().flush().unwrap();
        self.parse_input();
    }

    fn parse_input(&mut self) {
        let mut input: Vec<Token> = Vec::new();
        let mut buffer = String::new();

        self.stdin.read_line(&mut buffer).unwrap();

        buffer.pop(); // get rid of \n at the end

        let mut buffer: Vec<&str> = buffer.split(' ').collect();

        while buffer[0].is_empty() && buffer.len() != 1 {
            buffer.remove(0);
        }

        // match each word with a token
        for word in buffer {
            input.push(
                match word {
                    ";" => Token::EndLine,
                    "||" => Token::IfFalse,
                    "&&" => Token::IfTrue,
                    "!" => Token::Negate,
                    "(" => Token::OpenBrace,
                    ")" => Token::CloseBrace,
                    _ => {
                        if let Some(Token::Command(_) | Token::Argument(_)) = input.last(){
                            Token::Argument(CString::new(word.to_owned()).unwrap())
                        } else {
                            Token::Command(word.clone())
                        }
                    }
                }
            )
        }

        let mut parsed_input: Vec<(Token, Vec<CString>)> = Vec::new();
        let mut tk: (Option<Token>, Vec<CString>) = (None, Vec::new());

        for token in input {
            match token {
                Token::Argument(v) => {
                    tk.1.push(v);
                },
                Token::Command(v) => {
                    if tk.0.is_some() {
                        parsed_input.push((tk.0.unwrap(), tk.1));
                        tk.0 = None;
                        tk.1 = Vec::new();
                    }
                    tk.0 = Some(token);
                    tk.1.push(CString::new(v).unwrap());
                }
                _ => {},
            }
        }

        parsed_input.push((tk.0.unwrap(), tk.1));
        println!("{:?}", parsed_input);

        for token in parsed_input {
            self.parse_token(token.0, token.1)
        }
    }

    fn parse_token(&mut self, token: Token, args: Vec<CString>) {
        if self.skip_next {
            self.skip_next = false;
            return
        }

        match token {
            Token::EndLine => return,
            Token::IfFalse => {
                self.skip_next = self.last_exit_status == 0;
                return
            },
            Token::IfTrue => {
                self.skip_next = self.last_exit_status != 0;
                return
            },
            Token::Negate => {
                self.flip_exit_status = true;
                return
            }
            _ => {},
        }

        if let Token::Command(v) = token {
            let mut args_cstr: Vec<CString> = Vec::new();
            let mut cloned_args = args.clone();
            while let Some(v) = cloned_args.pop() {
                args_cstr.push(v);
            }
            args_cstr.reverse();
            match v { // first buffer match - for stuff that doesn't require/can't happen in a fork
                "||" |
                "cd" |
                "exec" => {
                    println!("{} {:?}", v, args_cstr);
                    self.execute_process(v, args_cstr);
                },
                _ => {
                    let pid = unsafe {fork()}.expect("fork failed");

                    match pid {
                        Child => {
                            self.execute_process(v, args_cstr);
                        },
                        Parent { child: _ } => {
                            let res = wait().unwrap();
                            match res {
                                WaitStatus::Exited(_, v) => {
                                    self.set_exit_status(v);
                                }
                                _ => {},
                            }
                        }
                    }
                }
            }
        }
    }
}

fn main() {
    let mut shell = Shell::new();
    loop {
        shell.run();
    }
}
