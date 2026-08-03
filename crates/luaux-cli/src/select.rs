//! Which files a build considers.
//!
//! `include` and `exclude` are matched against the path *relative to the source
//! root*. A pattern with no `/` matches at any depth, as in gitignore, so
//! `"*.spec.luaux"` does what it looks like rather than only matching at the
//! top level.

use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::Path;

pub struct Selector {
    include: GlobSet,
    exclude: GlobSet,
}

impl Selector {
    pub fn new(include: &[String], exclude: &[String]) -> Result<Self, String> {
        Ok(Self {
            include: build(include)?,
            exclude: build(exclude)?,
        })
    }

    pub fn allows(&self, relative: &Path) -> bool {
        self.include.is_match(relative) && !self.exclude.is_match(relative)
    }
}

fn build(patterns: &[String]) -> Result<GlobSet, String> {
    let mut builder = GlobSetBuilder::new();

    for pattern in patterns {
        // gitignore semantics: a bare name matches at any depth.
        let rooted = if pattern.contains('/') {
            pattern.clone()
        } else {
            format!("**/{pattern}")
        };

        let glob =
            Glob::new(&rooted).map_err(|error| format!("bad pattern {pattern:?}: {error}"))?;
        builder.add(glob);

        // `**` alone should also match a top-level file, which the `**/` form
        // above does not cover on its own.
        if pattern == "**" {
            builder.add(Glob::new("*").expect("literal"));
        }
    }

    builder
        .build()
        .map_err(|error| format!("bad pattern set: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selector(include: &[&str], exclude: &[&str]) -> Selector {
        Selector::new(
            &include.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            &exclude.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
        .expect("patterns")
    }

    #[test]
    fn the_default_include_matches_everything() {
        let selector = selector(&["**"], &[]);
        assert!(selector.allows(Path::new("App.luaux")));
        assert!(selector.allows(Path::new("ui/Button.luaux")));
        assert!(selector.allows(Path::new("a/b/c/Deep.luaux")));
    }

    #[test]
    fn a_bare_pattern_matches_at_any_depth() {
        let selector = selector(&["**"], &["*.spec.luaux"]);
        assert!(!selector.allows(Path::new("App.spec.luaux")));
        assert!(!selector.allows(Path::new("ui/Button.spec.luaux")));
        assert!(selector.allows(Path::new("ui/Button.luaux")));
    }

    #[test]
    fn a_rooted_pattern_stays_rooted() {
        let selector = selector(&["**"], &["vendor/**"]);
        assert!(!selector.allows(Path::new("vendor/thing.luaux")));
        assert!(selector.allows(Path::new("ui/vendor.luaux")));
    }

    #[test]
    fn include_can_narrow() {
        let selector = selector(&["ui/**"], &[]);
        assert!(selector.allows(Path::new("ui/Button.luaux")));
        assert!(!selector.allows(Path::new("other/Button.luaux")));
    }

    #[test]
    fn exclude_beats_include() {
        let selector = selector(&["ui/**"], &["ui/internal/**"]);
        assert!(selector.allows(Path::new("ui/Button.luaux")));
        assert!(!selector.allows(Path::new("ui/internal/Thing.luaux")));
    }
}
