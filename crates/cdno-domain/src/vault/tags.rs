//! `Vault::notes_tagged` — a tag query that follows a note's former names
//! (RFC 0007 §5.5, "Tags").

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
    /// written before the rename must still find it. So when `<slug>` is the
    /// current slug of a note, `<word>/<alias>` is queried for each of that
    /// note's aliases; when no note has that slug but one declares it as an
    /// alias (a stem beats an alias, as in link resolution), `<word>/<current-slug>`
    /// and its other aliases are queried too. The index stays a plain lookup
    /// by exact tag; the expansion is done here. The result is the union,
    /// each path once, in path order. A tag without a `/`, or whose tail is
    /// not slug-shaped, is passed straight to the index.
    ///
    /// No CLI or MCP surface calls this yet.
    pub fn notes_tagged(&self, tag: &str) -> Result<Vec<VaultPath>, DomainError> {
        let Some((word, slug)) = tag.split_once('/').filter(|(_, s)| is_slug_shaped(s)) else {
            return Ok(self.index.find_by_tag(tag)?);
        };

        // The names the tag's note answers to, the queried one first.
        let mut names: Vec<String> = vec![slug.to_owned()];
        let mut push_all = |more: Vec<String>| {
            for n in more {
                if !names.contains(&n) {
                    names.push(n);
                }
            }
        };
        let current: Vec<VaultPath> = self
            .index
            .list_all_paths()?
            .into_iter()
            .filter(|p| candidate_slug(p).as_deref() == Some(slug))
            .collect();
        if current.is_empty() {
            for holder in self.alias_holders(slug, |_| true)? {
                push_all(candidate_slug(&holder).into_iter().collect());
                push_all(self.aliases_of(&holder)?);
            }
        } else {
            for path in &current {
                push_all(self.aliases_of(path)?);
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
