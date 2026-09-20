//! The core of rgrep: pattern matching, single-file search and recursive
//! directory search, kept apart from `main` so all of it is testable
//! without spawning a process.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Lines of `contents` that contain `pattern` as a literal substring.
pub fn matching_lines<'a>(pattern: &str, contents: &'a str) -> Vec<&'a str> {
    matches(pattern, contents, false)
}

/// Lines of `contents` that contain `pattern` as a literal substring, case
/// differences ignored.
///
/// Folding is ASCII-only (`'A'..='Z'` against `'a'..='z'`): `"Straße"` and
/// `"STRASSE"`, or `"café"` and `"CAFÉ"`, do not match one another here,
/// since folding an accented letter needs full Unicode case folding, which
/// this function does not attempt.
pub fn matching_lines_ignoring_case<'a>(pattern: &str, contents: &'a str) -> Vec<&'a str> {
    matches(pattern, contents, true)
}

fn matches<'a>(pattern: &str, contents: &'a str, ignore_case: bool) -> Vec<&'a str> {
    if ignore_case {
        let pattern = pattern.to_ascii_lowercase();
        contents
            .lines()
            .filter(|line| line.to_ascii_lowercase().contains(&pattern))
            .collect()
    } else {
        contents
            .lines()
            .filter(|line| line.contains(pattern))
            .collect()
    }
}

/// Reads `path` and returns the lines that contain `pattern`.
pub fn search_file(pattern: &str, path: &str) -> Result<Vec<String>, String> {
    search_file_matching(pattern, path, false)
}

/// Reads `path` and returns the lines that contain `pattern`, case
/// differences ignored (see [`matching_lines_ignoring_case`]).
pub fn search_file_ignoring_case(pattern: &str, path: &str) -> Result<Vec<String>, String> {
    search_file_matching(pattern, path, true)
}

fn search_file_matching(
    pattern: &str,
    path: &str,
    ignore_case: bool,
) -> Result<Vec<String>, String> {
    let contents = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    Ok(matches(pattern, &contents, ignore_case)
        .into_iter()
        .map(str::to_owned)
        .collect())
}

/// Every regular file found under `root`, descending into subdirectories,
/// in a deterministic (sorted) order.
///
/// A directory entry is classified by `DirEntry::file_type`, which reads
/// the entry itself rather than what it points at, so a symlink is neither
/// a file nor a directory here and is skipped — the simplest way to never
/// follow one into a cycle.
pub fn walk_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let read_dir = fs::read_dir(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let mut entries = Vec::new();
    for entry in read_dir {
        entries.push(entry.map_err(|e| format!("{}: {e}", root.display()))?);
    }
    entries.sort_by_key(fs::DirEntry::path);

    let mut files = Vec::new();
    for entry in entries {
        let file_type = entry
            .file_type()
            .map_err(|e| format!("{}: {e}", entry.path().display()))?;
        if file_type.is_dir() {
            files.extend(walk_files(&entry.path())?);
        } else if file_type.is_file() {
            files.push(entry.path());
        }
    }
    Ok(files)
}

/// Searches `path` for `pattern`: a plain search when `path` is a file, one
/// line per match exactly as `search_file` gives it; a recursive descent
/// when `path` is a directory, one `"file:line"` per match so a line can be
/// traced back to the file it came from.
pub fn search_path(pattern: &str, path: &str) -> Result<Vec<String>, String> {
    search_path_matching(pattern, path, false)
}

/// Searches `path` for `pattern` as [`search_path`] does, case differences
/// ignored (see [`matching_lines_ignoring_case`]).
pub fn search_path_ignoring_case(pattern: &str, path: &str) -> Result<Vec<String>, String> {
    search_path_matching(pattern, path, true)
}

fn search_path_matching(
    pattern: &str,
    path: &str,
    ignore_case: bool,
) -> Result<Vec<String>, String> {
    let root = Path::new(path);
    let metadata = fs::metadata(root).map_err(|e| format!("{path}: {e}"))?;
    if !metadata.is_dir() {
        return search_file_matching(pattern, path, ignore_case);
    }

    let mut found = Vec::new();
    for file in walk_files(root)? {
        let contents = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        for line in matches(pattern, &contents, ignore_case) {
            found.push(format!("{}:{line}", file.display()));
        }
    }
    Ok(found)
}

/// Runs rgrep over `args` (`[-i] <pattern> <path>`), writing matches to
/// `stdout` and problems to `stderr`. `path` may be a file or a directory,
/// searched recursively. `-i`, when it is the first argument, makes the
/// search ignore ASCII case (see [`matching_lines_ignoring_case`]); any
/// other argument starting with `-` is refused by name rather than taken
/// for the pattern.
///
/// The exit code follows grep's own convention: 0 when a line matched, 1
/// when none did, 2 when the run could not even be attempted.
pub fn run(args: &[String], stdout: &mut impl Write, stderr: &mut impl Write) -> u8 {
    let (ignore_case, rest) = match args.split_first() {
        Some((flag, rest)) if flag == "-i" => (true, rest),
        Some((flag, _)) if flag.starts_with('-') => {
            writeln!(stderr, "rgrep: unknown flag: {flag}").ok();
            return 2;
        }
        _ => (false, args),
    };

    let (pattern, path) = match rest {
        [pattern, path] => (pattern, path),
        _ => {
            writeln!(stderr, "usage: rgrep <pattern> <file-or-directory>").ok();
            return 2;
        }
    };

    let result = if ignore_case {
        search_path_ignoring_case(pattern, path)
    } else {
        search_path(pattern, path)
    };

    match result {
        Ok(lines) => {
            for line in &lines {
                writeln!(stdout, "{line}").ok();
            }
            u8::from(lines.is_empty())
        }
        Err(e) => {
            writeln!(stderr, "rgrep: {e}").ok();
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_file(contents: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("rgrep-test-{}-{n}", std::process::id()));
        fs::write(&path, contents).expect("write temp file");
        path
    }

    fn temp_dir() -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("rgrep-test-dir-{}-{n}", std::process::id()));
        fs::create_dir(&path).expect("create temp dir");
        path
    }

    #[test]
    fn matching_lines_keeps_only_lines_with_the_pattern() {
        let text = "apple\nbanana\napplesauce\ncherry\n";
        assert_eq!(matching_lines("apple", text), vec!["apple", "applesauce"]);
    }

    #[test]
    fn matching_lines_is_empty_when_nothing_matches() {
        let text = "one\ntwo\nthree\n";
        assert!(matching_lines("xyz", text).is_empty());
    }

    #[test]
    fn matching_lines_is_case_sensitive() {
        let text = "Apple\napple\n";
        assert_eq!(matching_lines("apple", text), vec!["apple"]);
    }

    #[test]
    fn search_file_reads_and_filters_a_real_file() {
        let path = temp_file("foo\nbar foo\nbaz\n");
        let result = search_file("foo", path.to_str().unwrap()).unwrap();
        assert_eq!(result, vec!["foo", "bar foo"]);
        fs::remove_file(path).ok();
    }

    #[test]
    fn search_file_errors_on_a_missing_file() {
        let result = search_file("foo", "/no/such/path/rgrep-missing-file");
        assert!(result.is_err());
    }

    #[test]
    fn walk_files_descends_into_subdirectories_in_sorted_order() {
        let dir = temp_dir();
        fs::write(dir.join("b.txt"), "b").unwrap();
        fs::create_dir(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/c.txt"), "c").unwrap();
        fs::write(dir.join("a.txt"), "a").unwrap();

        let files = walk_files(&dir).unwrap();

        assert_eq!(
            files,
            vec![dir.join("a.txt"), dir.join("b.txt"), dir.join("sub/c.txt")]
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn walk_files_is_empty_for_an_empty_directory() {
        let dir = temp_dir();
        assert!(walk_files(&dir).unwrap().is_empty());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn walk_files_errors_when_the_root_does_not_exist() {
        assert!(walk_files(Path::new("/no/such/rgrep-missing-dir")).is_err());
    }

    #[test]
    fn search_path_searches_every_file_under_a_directory_and_prefixes_matches() {
        let dir = temp_dir();
        fs::write(dir.join("one.txt"), "foo\nbar\n").unwrap();
        fs::create_dir(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/two.txt"), "foo again\nnothing here\n").unwrap();

        let mut matches = search_path("foo", dir.to_str().unwrap()).unwrap();
        matches.sort();

        let mut expected = vec![
            format!("{}:foo", dir.join("one.txt").display()),
            format!("{}:foo again", dir.join("sub/two.txt").display()),
        ];
        expected.sort();
        assert_eq!(matches, expected);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn search_path_is_empty_when_no_file_under_the_directory_matches() {
        let dir = temp_dir();
        fs::write(dir.join("one.txt"), "bar\n").unwrap();
        assert!(
            search_path("foo", dir.to_str().unwrap())
                .unwrap()
                .is_empty()
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn search_path_falls_back_to_plain_search_for_a_single_file() {
        let path = temp_file("foo\nbar foo\nbaz\n");
        let result = search_path("foo", path.to_str().unwrap()).unwrap();
        assert_eq!(result, vec!["foo", "bar foo"]);
        fs::remove_file(path).ok();
    }

    #[test]
    fn search_path_errors_when_nothing_exists_at_all() {
        assert!(search_path("foo", "/no/such/path/rgrep-missing-anything").is_err());
    }

    #[test]
    fn run_prints_matching_lines_and_reports_success() {
        let path = temp_file("hello\nworld\nhello world\n");
        let args = vec!["hello".to_string(), path.to_str().unwrap().to_string()];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = run(&args, &mut stdout, &mut stderr);

        assert_eq!(code, 0);
        assert_eq!(String::from_utf8(stdout).unwrap(), "hello\nhello world\n");
        assert!(stderr.is_empty());
        fs::remove_file(path).ok();
    }

    #[test]
    fn run_reports_no_match_with_exit_code_one_and_no_output() {
        let path = temp_file("hello\nworld\n");
        let args = vec!["xyz".to_string(), path.to_str().unwrap().to_string()];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = run(&args, &mut stdout, &mut stderr);

        assert_eq!(code, 1);
        assert!(stdout.is_empty());
        fs::remove_file(path).ok();
    }

    #[test]
    fn run_reports_exit_code_two_for_a_missing_file() {
        let args = vec![
            "foo".to_string(),
            "/no/such/path/rgrep-missing-file".to_string(),
        ];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = run(&args, &mut stdout, &mut stderr);

        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        let message = String::from_utf8(stderr).unwrap();
        assert!(message.starts_with("rgrep: /no/such/path/rgrep-missing-file:"));
    }

    #[test]
    fn run_reports_usage_for_the_wrong_number_of_arguments() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = run(&["only-one".to_string()], &mut stdout, &mut stderr);

        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "usage: rgrep <pattern> <file-or-directory>\n"
        );
    }

    #[test]
    fn run_reports_usage_for_no_arguments_at_all() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = run(&[], &mut stdout, &mut stderr);

        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "usage: rgrep <pattern> <file-or-directory>\n"
        );
    }

    #[test]
    fn run_reports_usage_for_too_many_arguments() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = run(
            &["a".to_string(), "b".to_string(), "c".to_string()],
            &mut stdout,
            &mut stderr,
        );

        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "usage: rgrep <pattern> <file-or-directory>\n"
        );
    }

    #[test]
    fn run_searches_a_directory_recursively_and_prefixes_each_matching_line() {
        let dir = temp_dir();
        fs::write(dir.join("one.txt"), "match here\nno\n").unwrap();
        fs::create_dir(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/two.txt"), "also a match\n").unwrap();

        let args = vec!["match".to_string(), dir.to_str().unwrap().to_string()];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = run(&args, &mut stdout, &mut stderr);

        assert_eq!(code, 0);
        let mut lines: Vec<&str> = std::str::from_utf8(&stdout).unwrap().lines().collect();
        lines.sort_unstable();
        let expected_one = format!("{}:match here", dir.join("one.txt").display());
        let expected_two = format!("{}:also a match", dir.join("sub/two.txt").display());
        let mut expected = vec![expected_one.as_str(), expected_two.as_str()];
        expected.sort_unstable();
        assert_eq!(lines, expected);
        assert!(stderr.is_empty());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn run_reports_no_match_for_a_directory_with_nothing_matching() {
        let dir = temp_dir();
        fs::write(dir.join("one.txt"), "nothing relevant\n").unwrap();

        let args = vec!["xyz".to_string(), dir.to_str().unwrap().to_string()];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = run(&args, &mut stdout, &mut stderr);

        assert_eq!(code, 1);
        assert!(stdout.is_empty());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn matching_lines_ignoring_case_matches_regardless_of_ascii_case() {
        let text = "Apple\napple\nAPPLE\nbanana\n";
        assert_eq!(
            matching_lines_ignoring_case("apple", text),
            vec!["Apple", "apple", "APPLE"]
        );
    }

    #[test]
    fn matching_lines_ignoring_case_is_still_empty_when_nothing_matches() {
        let text = "one\ntwo\nthree\n";
        assert!(matching_lines_ignoring_case("XYZ", text).is_empty());
    }

    #[test]
    fn matching_lines_ignoring_case_does_not_fold_non_ascii_case() {
        // "café" and "CAFÉ" differ only in the accented letter's case:
        // ASCII-only folding leaves accented letters untouched, so they
        // must not match here even though the flag is set.
        let text = "CAFÉ\n";
        assert!(matching_lines_ignoring_case("café", text).is_empty());
    }

    #[test]
    fn search_file_ignoring_case_reads_and_filters_a_real_file() {
        let path = temp_file("Foo\nbar FOO\nbaz\n");
        let result = search_file_ignoring_case("foo", path.to_str().unwrap()).unwrap();
        assert_eq!(result, vec!["Foo", "bar FOO"]);
        fs::remove_file(path).ok();
    }

    #[test]
    fn search_path_ignoring_case_searches_every_file_under_a_directory() {
        let dir = temp_dir();
        fs::write(dir.join("one.txt"), "Foo\nbar\n").unwrap();
        fs::create_dir(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/two.txt"), "FOO again\nnothing here\n").unwrap();

        let mut matches = search_path_ignoring_case("foo", dir.to_str().unwrap()).unwrap();
        matches.sort();

        let mut expected = vec![
            format!("{}:Foo", dir.join("one.txt").display()),
            format!("{}:FOO again", dir.join("sub/two.txt").display()),
        ];
        expected.sort();
        assert_eq!(matches, expected);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn run_with_i_flag_matches_case_insensitively() {
        let path = temp_file("Hello\nWORLD\nhello world\n");
        let args = vec![
            "-i".to_string(),
            "hello".to_string(),
            path.to_str().unwrap().to_string(),
        ];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = run(&args, &mut stdout, &mut stderr);

        assert_eq!(code, 0);
        assert_eq!(String::from_utf8(stdout).unwrap(), "Hello\nhello world\n");
        assert!(stderr.is_empty());
        fs::remove_file(path).ok();
    }

    #[test]
    fn run_without_i_flag_stays_case_sensitive() {
        let path = temp_file("Hello\nhello\n");
        let args = vec!["hello".to_string(), path.to_str().unwrap().to_string()];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = run(&args, &mut stdout, &mut stderr);

        assert_eq!(code, 0);
        assert_eq!(String::from_utf8(stdout).unwrap(), "hello\n");
        fs::remove_file(path).ok();
    }

    #[test]
    fn run_refuses_an_unknown_flag_and_names_it() {
        let args = vec!["-x".to_string(), "needle".to_string(), ".".to_string()];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = run(&args, &mut stdout, &mut stderr);

        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "rgrep: unknown flag: -x\n"
        );
    }
}
