use {Instance, Config};

use tool;
use std;

use regex::{Regex, Captures};

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct Directive
{
    pub command: Command,
    pub line: u32,
}

#[derive(Clone,Debug)]
pub enum Command
{
    Run(tool::Invocation),
    Check(Regex),
    CheckNext(Regex),
}

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct Test
{
    pub path: String,
    pub directives: Vec<Directive>,
}

#[derive(Clone,Debug,PartialEq,Eq)]
pub enum TestResultKind
{
    Pass,
    Fail(String, String),
    Skip,
}

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct TestResult
{
    pub path: String,
    pub kind: TestResultKind,
}

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct Context
{
    pub exec_search_dirs: Vec<String>,
    pub tests: Vec<Test>,
}

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct Results
{
    test_results: Vec<TestResult>,
}

impl Directive
{
    pub fn new(command: Command, line: u32) -> Self {
        Directive {
            command: command,
            line: line,
        }
    }

    /// Converts a match string to a regex.
    ///
    /// Converts from the `[[capture_name:capture_regex]]` syntaxs to
    /// a regex.
    fn parse_regex(mut string: String) -> Regex {
        let capture_regex = Regex::new("\\[\\[(\\w+):(.*)\\]\\]").unwrap();

        if capture_regex.is_match(&string) {
            string = capture_regex.replace_all(&string, |caps: &Captures| {
                format!("(?P<{}>{})", caps.get(1).unwrap().as_str(), caps.get(2).unwrap().as_str())
            }).into_owned();
        }

        Regex::new(&string).unwrap()
    }

    pub fn is_directive(string: &str) -> bool {
        // KEEP UP TO DATE WITH maybe_parse
        // FIXME: make this a lazy static
        let regex = Regex::new("([A-Z-]+):(.*)").unwrap();
        regex.is_match(string)
    }

    pub fn maybe_parse(string: &str, line: u32, config: &Config) -> Option<Result<Self,String>> {
        // KEEP UP TO DATE WITH maybe_parse
        let regex = Regex::new("([A-Z-]+):(.*)").unwrap();

        if !regex.is_match(string) { return None; }

        let captures = regex.captures(string).unwrap();
        let command_str = captures.get(1).unwrap().as_str().trim();
        let after_command_str = captures.get(2).unwrap().as_str().trim();

        match command_str {
            // FIXME: better message if we have 'RUN :'
            "RUN" => {
                let inner_words = after_command_str.split_whitespace();
                let invocation = match tool::Invocation::parse(inner_words, config) {
                    Ok(i) => i,
                    Err(e) => return Some(Err(e)),
                };

                Some(Ok(Directive::new(Command::Run(invocation), line)))
            },
            "CHECK" => {
                let regex = Self::parse_regex(after_command_str.to_owned());
                Some(Ok(Directive::new(Command::Check(regex), line)))
            },
            "CHECK-NEXT" => {
                let regex = Self::parse_regex(after_command_str.to_owned());
                Some(Ok(Directive::new(Command::CheckNext(regex), line)))
            },
            _ => {
                Some(Err(format!("command '{}' not known", command_str)))
            },
        }
    }
}

impl Test
{
    pub fn parse<S,I>(name: S, chars: I, config: &Config) -> Result<Self,String>
        where S: Into<String>, I: Iterator<Item=char> {
        let mut directives = Vec::new();
        let test_body: String = chars.collect();

        for (line_idx, line) in test_body.lines().enumerate() {
            let line_number = line_idx + 1;

            match Directive::maybe_parse(line, line_number as _, config) {
                Some(Ok(directive)) => directives.push(directive),
                Some(Err(e)) => {
                    return Err(format!(
                        "could not parse directive: {}", e)
                    );
                },
                None => continue,
            }
        }

        Ok(Test {
            path: name.into(),
            directives: directives,
        })
    }

    pub fn run(&self, context: &Context, config: &Config) -> TestResult {
        if self.is_empty() {
            return TestResult {
                path: self.path.clone(),
                kind: TestResultKind::Skip,
            }
        }

        for instance in self.instances() {
            let kind = instance.run(self, context, config);

            match kind {
                TestResultKind::Pass => continue,
                TestResultKind::Skip => {
                    return TestResult {
                        path: self.path.clone(),
                        kind: TestResultKind::Pass,
                    }
                },
                TestResultKind::Fail(msg, desc) => {
                    return TestResult {
                        path: self.path.clone(),
                        kind: TestResultKind::Fail(msg, desc),
                    }
                },
            }
        }

        TestResult {
            path: self.path.clone(),
            kind: TestResultKind::Pass,
        }
    }

    pub fn instances(&self) -> Vec<Instance> {
        self.directives.iter().flat_map(|directive| {
            if let Command::Run(ref invocation) = directive.command {
                Some(Instance::new(invocation.clone()))
            } else {
                None
            }
        }).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.directives.is_empty()
    }
}

impl TestResultKind
{
    pub fn fail<S: Into<String>>(s: S) -> Self {
        TestResultKind::Fail(s.into(), "".to_owned())
    }
}

impl Context
{
    pub fn new() -> Self {
        Context {
            exec_search_dirs: Vec::new(),
            tests: Vec::new(),
        }
    }

    pub fn test(mut self, test: Test) -> Self {
        self.tests.push(test);
        self
    }

    pub fn run(&self, config: &Config) -> Results {
        let test_results = self.tests.iter().map(|test| {
            test.run(self, config)
        }).collect();

        Results {
            test_results: test_results,
        }
    }

    pub fn add_search_dir(&mut self, dir: String) {
        self.exec_search_dirs.push(dir);
    }

    pub fn find_in_search_dir(&self, path: &str)
        -> Option<String> {
        for dir in self.exec_search_dirs.iter() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                let cur_path = entry.path();

                if std::fs::metadata(&cur_path).unwrap().is_file() {
                    if cur_path.file_name().unwrap() == path {
                        return Some(cur_path.to_str().unwrap().to_owned());
                    }
                }
            }
        }
        None
    }

    pub fn executable_path(&self, path: &str, config: &Config) -> String {
        // Check if executable is a variable.
        if path.starts_with("@") {
            let value = config.constants.get(&path[1..]).expect("no constant with that name found");
            return value.to_owned();
        }

        match self.find_in_search_dir(path) {
            Some(p) => p,
            None => path.to_owned(),
        }
    }
}

impl Results
{
    pub fn test_results(&self) -> ::std::slice::Iter<TestResult> {
        self.test_results.iter()
    }

    pub fn iter(&self) -> ::std::slice::Iter<TestResult> {
        self.test_results()
    }
}

impl PartialEq for Command {
    fn eq(&self, other: &Command) -> bool {
        match *self {
            Command::Run(ref a) => if let Command::Run(ref b) = *other { a == b } else { false },
            Command::Check(ref a) => if let Command::Check(ref b) = *other { a.to_string() == b.to_string() } else { false },
            Command::CheckNext(ref a) => if let Command::CheckNext(ref b) = *other { a.to_string() == b.to_string() } else { false },

        }
    }
}

impl Eq for Command { }

#[cfg(test)]
mod test {
    use super::*;
    use Config;

    fn parse(line: &str) -> Result<Directive, String> {
        Directive::maybe_parse(line, 0, &Config::default()).unwrap()
    }

    #[test]
    fn can_parse_run() {
        let _d = parse("; RUN: foo").unwrap();
    }
}
