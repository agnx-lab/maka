/*
 * Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.  See the NOTICE file
 * distributed with this work for additional information
 * regarding copyright ownership.  The ASF licenses this file
 * to you under the Apache License, Version 2.0 (the
 * "License"); you may not use this file except in compliance
 * with the License.  You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
 * KIND, either express or implied.  See the License for the
 * specific language governing permissions and limitations
 * under the License.
 */

//! Original whole-paste local path grammar; no filesystem access or shell evaluation.
use std::path::{Path, PathBuf};

pub(in crate::application) fn valid(path: &Path) -> bool {
    path.is_absolute()
        && path
            .to_str()
            .is_some_and(|text| text.len() <= 4096 && !text.chars().any(char::is_control))
}

/// Interpret only a whole paste made of local paths; never run shell expansion.
pub(in crate::application) fn parse(text: &str) -> Option<Vec<PathBuf>> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let words = path_words(text, cfg!(windows))?;
    let parsed: Vec<_> = words
        .iter()
        .map(|word| local_path(word))
        .collect::<Option<_>>()?;
    (!parsed.is_empty()).then_some(parsed)
}

/// Path quoting, not a shell language: comments, operators and expansions stay
/// literal tokens, so a prose suffix can never disappear during classification.
fn path_words(text: &str, windows: bool) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut started = false;
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        if quote.is_none() && word.starts_with("file://") && !character.is_whitespace() {
            // URI apostrophes are path characters, not shell quote delimiters.
            word.push(character);
        } else if quote == Some('\'') {
            if character == '\'' {
                quote = None;
            } else {
                word.push(character);
            }
        } else if character == '\\' && !windows {
            let escaped = chars.next()?;
            if matches!(escaped, '\n' | '\r') {
                return None;
            }
            if quote == Some('"') && !matches!(escaped, '\\' | '"' | '$' | '`') {
                word.push('\\');
            }
            word.push(escaped);
            started = true;
        } else if Some(character) == quote {
            quote = None;
        } else if quote.is_none() && (character == '"' || character == '\'' && !windows) {
            quote = Some(character);
            started = true;
        } else if quote.is_none() && character.is_whitespace() {
            if started {
                words.push(std::mem::take(&mut word));
                started = false;
            }
        } else {
            word.push(character);
            started = true;
        }
    }
    if started {
        words.push(word);
    }
    quote.is_none().then_some(words)
}
pub(in crate::application) fn local_path(text: &str) -> Option<PathBuf> {
    let path = if text.starts_with("file://") {
        if text.chars().any(char::is_whitespace) {
            return None;
        }
        let url = url::Url::parse(text).ok()?;
        if url.host_str().is_some_and(|host| host != "localhost")
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return None;
        }
        url.to_file_path().ok()?
    } else {
        PathBuf::from(text)
    };
    valid(&path).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nine_individually_valid_paths_are_parsed_without_an_aggregate_count_budget() {
        let expected: Vec<_> = (0..9)
            .map(|index| {
                let prefix = if cfg!(windows) { r"C:\" } else { "/" };
                PathBuf::from(format!("{prefix}{}{index}.txt", "a".repeat(3700)))
            })
            .collect();
        assert!(expected.iter().all(|path| valid(path)));
        let text = expected
            .iter()
            .map(|path| format!("\"{}\"", path.display()))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.len() > 32768);
        assert_eq!(parse(&text), Some(expected));
    }
    #[test]
    fn path_pastes_preserve_unicode_spaces_and_never_evaluate_shell_syntax() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("中文 image.png");
        let second = directory.path().join("it's a file.txt");
        let quoted = format!("\"{}\" \"{}\"", first.display(), second.display());
        #[cfg(not(windows))]
        assert_eq!(parse(&quoted), Some(vec![first.clone(), second.clone()]));
        let uri_list = format!(
            "{}\r\n{}",
            url::Url::from_file_path(&first).unwrap(),
            url::Url::from_file_path(&second).unwrap()
        );
        assert_eq!(parse(&uri_list), Some(vec![first.clone(), second.clone()]));
        assert_eq!(
            parse(&uri_list.replace("\r\n", " ")),
            Some(vec![first.clone(), second.clone()])
        );
        assert!(
            parse(first.to_str().unwrap()).is_none(),
            "unquoted prose is not guessed to be a path containing spaces"
        );
        for text in [
            "Explain /tmp/file.txt",
            "hello\nworld",
            "file://remote.example/file.txt",
            "file:///tmp/a%00b",
            "$(touch /tmp/not-an-operation)",
            "relative.txt",
            "/tmp/a.txt # explain this file",
            "/tmp/a.txt\n# notes",
            "file:///tmp/a.txt please explain",
            "\"file:///tmp/a b\"",
            "/tmp/a.txt \"\"",
        ] {
            assert!(parse(text).is_none(), "{text}");
        }
        #[cfg(not(windows))]
        for text in ["'/tmp/file # name.txt'", r"/tmp/file\ \#\ name.txt"] {
            assert_eq!(
                parse(text),
                Some(vec![PathBuf::from("/tmp/file # name.txt")])
            );
        }
        assert_eq!(
            path_words(r#""C:\My Files\a.png" "D:\中文\b.txt""#, true),
            Some(vec![r"C:\My Files\a.png".into(), r"D:\中文\b.txt".into()])
        );
    }
}
