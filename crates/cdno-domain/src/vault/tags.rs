//! `Vault::notes_tagged` — a tag query that follows a note's former names
//! (RFC 0007 §5.5, "Tags").

use std::collections::HashSet;

use cdno_core::path::VaultPath;
use cdno_core::slug::is_slug_shaped;

use crate::error::DomainError;

use super::Vault;
use super::note_ref::candidate_slug;

impl Vault {
    /// The notes carrying `tag`, and, for a `<word>/<slug>` tag such as
    /// `action/surrogate-model`, those carrying the same tag under any other
    /// name of the note `<slug>` names.
    ///
    /// A note renamed `old` to `new` keeps `old` as an alias, and a `#action/old`
    /// written before the rename must still find it. "The note `<slug>` names"
    /// and "the names a note answers to" follow the rule a wikilink uses:
    ///
    /// - A note answers to its current slug, and to each alias that is not
    ///   the current slug or file stem of any indexed note (a stem beats an
    ///   alias) and that no other note declares (a shared alias resolves to
    ///   nothing).
    /// - `<slug>` names the one note whose current slug it is; if any note
    ///   has that slug, aliases are not consulted, and if several do (a stem
    ///   shared across folders) it names nothing. Otherwise it names the one
    ///   note that declares it as an alias. With no such note, or several, it
    ///   names nothing and only the literal tag is queried.
    ///
    /// `<word>` is not interpreted, and the expansion is type-blind, as
    /// wikilink resolution is. The index stays a plain lookup by exact tag;
    /// the expansion is done here. The result is the union, each path once,
    /// in path order. A tag without a `/`, or whose tail is not slug-shaped,
    /// is passed straight to the index.
    ///
    /// No CLI or MCP surface calls this yet.
    pub fn notes_tagged(&self, tag: &str) -> Result<Vec<VaultPath>, DomainError> {
        let Some((word, slug)) = tag.split_once('/').filter(|(_, s)| is_slug_shaped(s)) else {
            return Ok(self.index.find_by_tag(tag)?);
        };

        // Every current slug and file stem: the names an alias cannot take.
        let mut stems: HashSet<String> = HashSet::new();
        let mut current: Vec<VaultPath> = Vec::new();
        for path in self.index.list_all_paths()? {
            if let Some(stem) = path.as_path().file_stem().and_then(|s| s.to_str()) {
                stems.insert(stem.to_owned());
            }
            if let Some(name) = candidate_slug(&path) {
                if name == slug {
                    current.push(path.clone());
                }
                stems.insert(name);
            }
        }
        if current.is_empty() {
            if let Some((holder, _)) = self.alias_of(slug, |_| true)? {
                current.push(holder);
            }
        } else if current.len() > 1 {
            // A shared stem is ambiguous: a wikilink to it resolves to none.
            current.clear();
        }

        // The names the tag's notes answer to, the queried one first.
        let mut names: Vec<String> = vec![slug.to_owned()];
        for path in &current {
            let mut answers: Vec<String> = candidate_slug(path).into_iter().collect();
            for alias in self.aliases_of(path)? {
                let only_holder = self
                    .alias_of(&alias, |_| true)?
                    .is_some_and(|(holder, _)| &holder == path);
                if !stems.contains(&alias) && only_holder {
                    answers.push(alias);
                }
            }
            for n in answers {
                if !names.contains(&n) {
                    names.push(n);
                }
            }
        }

        let mut out: Vec<VaultPath> = Vec::new();
        for name in &names {
            for path in self.index.find_by_tag(&format!("{word}/{name}"))? {
                if !out.contains(&path) {
                    out.push(path);
                }
            }
        }
        // One order whatever the alias order: path text, as SQLite sorts it.
        out.sort_by_cached_key(|p| p.to_string());
        Ok(out)
    }
}
