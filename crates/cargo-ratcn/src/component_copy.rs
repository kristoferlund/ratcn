use std::path::Path;

/// One component module as a consumer would paste it into their own crate.
///
/// The test module goes: a copied module's tests reach the engine internals that
/// `ratcn`'s own suite covers, and the copy only needs to compile. Inner doc
/// comments become ordinary comments because a copied module can be included
/// into another target, where an inner attribute is invalid.
///
/// Everything from the test module down is dropped. A private-API or sibling
/// reference must be above `#[cfg(test)] mod tests` for this transform to check
/// it.
pub fn copy_of(source: &str, source_path: &Path) -> Result<String, String> {
    let mut lines: Vec<&str> = source.lines().collect();
    if let Some(tests) = lines.iter().position(|line| line.starts_with("mod tests")) {
        if tests == 0 || lines[tests - 1] != "#[cfg(test)]" {
            return Err(format!(
                "{}: the tests module is not preceded by #[cfg(test)]",
                source_path.display()
            ));
        }
        lines.truncate(tests - 1);
    }

    let mut copy = String::with_capacity(source.len());
    for line in lines {
        if line.starts_with("#![") {
            return Err(format!(
                "{}: an inner attribute cannot survive the copy — this crate includes each \
                 generated module into the target that compiles it",
                source_path.display()
            ));
        }
        let (prefix, body) = match line.strip_prefix("//!") {
            Some(rest) => ("//", rest),
            None => ("", line),
        };
        copy.push_str(prefix);
        copy.push_str(&rewrite_crate_paths(body));
        copy.push('\n');
    }
    Ok(copy)
}

/// Point `crate::` paths at the published crate, leaving identifiers that merely
/// end in `crate` (`some_crate::`) alone.
fn rewrite_crate_paths(line: &str) -> String {
    const NEEDLE: &str = "crate::";

    let mut rewritten = String::with_capacity(line.len());
    let mut cursor = 0;
    while let Some(offset) = line[cursor..].find(NEEDLE) {
        let start = cursor + offset;
        rewritten.push_str(&line[cursor..start]);
        let inside_identifier = line[..start]
            .chars()
            .next_back()
            .is_some_and(|character| character.is_alphanumeric() || character == '_');
        rewritten.push_str(if inside_identifier { NEEDLE } else { "ratcn::" });
        cursor = start + NEEDLE.len();
    }
    rewritten.push_str(&line[cursor..]);
    rewritten
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::copy_of;

    #[test]
    fn copies_only_the_consumer_facing_module() {
        let source = "//! Uses crate::Cycle.\nuse crate::Theme;\n#[cfg(test)]\nmod tests {\n    use crate::private_test_helper;\n}\nconst ALSO_DROPPED: () = ();\n";

        assert_eq!(
            copy_of(source, Path::new("cycle.rs")).expect("the source should transform"),
            "// Uses ratcn::Cycle.\nuse ratcn::Theme;\n"
        );
    }

    #[test]
    fn rewrites_only_standalone_crate_paths() {
        let source = "use crate::Theme;\nuse some_crate::Theme;\nuse _crate::Theme;\n";

        assert_eq!(
            copy_of(source, Path::new("component.rs")).expect("the source should transform"),
            "use ratcn::Theme;\nuse some_crate::Theme;\nuse _crate::Theme;\n"
        );
    }

    #[test]
    fn rejects_a_test_module_without_the_marker() {
        let error = copy_of("mod tests {}\n", Path::new("component.rs"))
            .expect_err("an unmarked test module cannot be copied");

        assert_eq!(
            error,
            "component.rs: the tests module is not preceded by #[cfg(test)]"
        );
    }

    #[test]
    fn rejects_inner_attributes_that_cannot_be_included() {
        let error = copy_of("#![allow(dead_code)]\n", Path::new("component.rs"))
            .expect_err("an inner attribute cannot be copied");

        assert!(error.starts_with("component.rs: an inner attribute cannot survive the copy"));
    }
}
