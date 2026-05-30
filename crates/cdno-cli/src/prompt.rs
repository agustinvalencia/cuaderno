//! Interactive prompts for CLI ergonomics — fuzzy selectors, text
//! input, and y/n confirms applied when a required arg is missing and
//! `stdout` is a TTY.
//!
//! Convention (see `docs/cli-ergonomics.md`):
//!
//! 1. Promptable args are declared `Option<T>` in clap.
//! 2. The handler folds each `Option` with [`is_interactive`] and
//!    [`missing_flag`]: `Some` → use; `None` + interactive → prompt;
//!    `None` + non-interactive → error.
//! 3. If any field was prompted, the handler renders a preview and
//!    calls [`confirm_preview`] before executing; all-Some runs
//!    straight through, matching the agentic (MCP / Tauri) shape.
//!
//! Only the helpers the action commands use ship in this PR; date /
//! status / milestone prompts arrive with the project / commit
//! retrofit (#114).

use std::io::IsTerminal;

use anyhow::{Result, anyhow};
use chrono::NaiveDate;
use inquire::{Confirm, DateSelect, Select, Text};

use cdno_domain::Vault;
use cdno_domain::frontmatter::{Context, EnergyLevel, QuestionDomain, QuestionStatus};
use cdno_domain::recurrence::Recurrence;

/// `true` when interactive prompts are available — `stdout` is a TTY
/// **and** the user hasn't opted out via `--no-interactive`. Handlers
/// use this to decide whether a missing `Option` argument turns into a
/// prompt or an error.
pub fn is_interactive(no_interactive: bool) -> bool {
    !no_interactive && std::io::stdout().is_terminal()
}

/// Build a clear "missing flag" error for the non-interactive path so
/// piped / scripted invocations fail fast rather than hanging.
pub fn missing_flag(flag: &str) -> anyhow::Error {
    anyhow!("missing required flag: --{flag} (provide it explicitly or run interactively in a TTY)")
}

/// Fuzzy-pick an active project. Returns the project slug.
///
/// Errors if there are no active projects — the user can't pick
/// nothing, and silently bailing would mask the real problem.
pub fn prompt_project(vault: &Vault) -> Result<String> {
    let active = vault.active_projects()?;
    if active.is_empty() {
        return Err(anyhow!(
            "no active projects — create one with `cdno project create`",
        ));
    }
    let labels: Vec<String> = active
        .iter()
        .map(|(path, fm)| {
            let slug = path
                .as_path()
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            format!("{slug} ({})", fm.context.as_str())
        })
        .collect();
    let pick = Select::new("Project", labels.clone()).prompt()?;
    let idx = labels
        .iter()
        .position(|l| l == &pick)
        .expect("picked label was in the offered list");
    let (path, _) = &active[idx];
    Ok(path
        .as_path()
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_owned())
}

/// Plain text input with `prompt` as the displayed label.
pub fn prompt_text(prompt: &str) -> Result<String> {
    Ok(Text::new(prompt).prompt()?)
}

/// Three-option energy selector matching the bullet suffix vocabulary.
pub fn prompt_energy() -> Result<EnergyLevel> {
    let choice = Select::new("Energy", vec!["deep", "medium", "light"]).prompt()?;
    Ok(match choice {
        "deep" => EnergyLevel::Deep,
        "medium" => EnergyLevel::Medium,
        "light" => EnergyLevel::Light,
        _ => unreachable!("Select only offers the three listed labels"),
    })
}

/// Y/N confirm with a default.
pub fn prompt_confirm(prompt: &str, default: bool) -> Result<bool> {
    Ok(Confirm::new(prompt).with_default(default).prompt()?)
}

/// Fuzzy-pick a bullet from a project's `## Next Actions` list. The
/// selected entry's `text` is returned so the domain's substring
/// matching resolves it uniquely on the next call.
///
/// `labels` should already include the energy suffix and any wikilink
/// — typically the `text` field of each [`cdno_domain::ActionListEntry`].
pub fn prompt_bullet(project: &str, labels: &[String]) -> Result<String> {
    if labels.is_empty() {
        return Err(anyhow!("no open actions on project '{project}'"));
    }
    Ok(Select::new("Bullet", labels.to_vec()).prompt()?)
}

/// Print a preview block and confirm before committing. Returns
/// `false` when the user declines (the caller should abort cleanly,
/// not error). Wraps inquire's confirm so the call site stays compact.
pub fn confirm_preview(preview: &str) -> Result<bool> {
    println!("{preview}");
    Ok(Confirm::new("Proceed?").with_default(true).prompt()?)
}

/// Fold a clap-optional value with the interactive / non-interactive
/// rule used by the ergonomics convention:
/// - `Some(v)` → return as-is.
/// - `None` + `interactive` → call `ask` and set `*prompted = true`.
/// - `None` + not interactive → return a clear missing-flag error.
///
/// Shared by every verb that follows the gather→confirm→execute
/// pattern (see `docs/cli-ergonomics.md`).
pub fn gather_or_error<T>(
    value: Option<T>,
    flag: &str,
    interactive: bool,
    prompted: &mut bool,
    ask: impl FnOnce() -> Result<T>,
) -> Result<T> {
    match value {
        Some(v) => Ok(v),
        None if interactive => {
            *prompted = true;
            ask()
        }
        None => Err(missing_flag(flag)),
    }
}

/// Calendar widget for picking an ISO date.
pub fn prompt_date(prompt: &str) -> Result<NaiveDate> {
    Ok(DateSelect::new(prompt).prompt()?)
}

/// Fuzzy-pick a *parked* project. Returns the project slug.
/// Mirrors [`prompt_project`] but limited to parked candidates — the
/// only valid input set for `cdno project activate`.
pub fn prompt_parked_project(vault: &Vault) -> Result<String> {
    let parked = vault.parked_projects()?;
    if parked.is_empty() {
        return Err(anyhow!(
            "no parked projects — `cdno project park <slug>` first",
        ));
    }
    let labels: Vec<String> = parked
        .iter()
        .map(|(path, fm)| {
            let slug = path
                .as_path()
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            format!("{slug} ({})", fm.context.as_str())
        })
        .collect();
    let pick = Select::new("Parked project", labels.clone()).prompt()?;
    let idx = labels
        .iter()
        .position(|l| l == &pick)
        .expect("picked label was in the offered list");
    let (path, _) = &parked[idx];
    Ok(path
        .as_path()
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_owned())
}

/// Fuzzy-pick an existing portfolio. Returns the portfolio slug.
/// Used by `cdno portfolio show` and `cdno file` for the
/// `--portfolio` field. `today` lets the picker's label include the
/// staleness in days.
pub fn prompt_portfolio(vault: &Vault, today: chrono::NaiveDate) -> Result<String> {
    let summaries = vault.list_portfolios(today)?;
    if summaries.is_empty() {
        return Err(anyhow!(
            "no portfolios \u{2014} create one with `cdno portfolio create`",
        ));
    }
    let labels: Vec<String> = summaries
        .iter()
        .map(|p| {
            let badge = match p.staleness_days {
                Some(days) if p.evidence_count > 0 => {
                    format!("{} evidence, {} days ago", p.evidence_count, days)
                }
                _ => "no evidence yet".to_owned(),
            };
            format!("{} ({badge})", p.slug)
        })
        .collect();
    let pick = Select::new("Portfolio", labels.clone()).prompt()?;
    let idx = labels
        .iter()
        .position(|l| l == &pick)
        .expect("picked label was in the offered list");
    Ok(summaries[idx].slug.clone())
}

/// Confirm whether a new milestone is a *hard* deadline. Default
/// `false` (soft target); the user opts into hard with `y`.
pub fn prompt_hard_soft() -> Result<bool> {
    prompt_confirm("Hard deadline? (counted in commitments aggregation)", false)
}

/// Fuzzy-pick an open milestone of a project. Returns the milestone
/// name so the domain's substring matcher resolves it uniquely.
pub fn prompt_open_milestone(project: &str, vault: &Vault) -> Result<String> {
    let open = vault.open_milestones(project)?;
    if open.is_empty() {
        return Err(anyhow!("no open milestones on project '{project}'"));
    }
    let labels: Vec<String> = open.iter().map(|m| m.name.clone()).collect();
    Ok(Select::new("Milestone", labels).prompt()?)
}

/// Pick a question's [`QuestionDomain`] from the enum's variants.
/// Used by `cdno question create` when `--domain` is omitted.
pub fn prompt_question_domain() -> Result<QuestionDomain> {
    let labels: Vec<&'static str> = QuestionDomain::ALL.iter().map(|d| d.as_str()).collect();
    let pick = Select::new("Domain", labels.clone()).prompt()?;
    let idx = labels
        .iter()
        .position(|l| l == &pick)
        .expect("picked label was in the offered list");
    Ok(QuestionDomain::ALL[idx])
}

/// Fuzzy-pick a question whose status is in `allow_statuses`.
/// Returns the question slug. `label` is the prompt shown ("Question
/// to park", "Question to activate", …) so the verb the user is
/// running is named explicitly.
///
/// Errors when no question matches the filter — the user can't pick
/// nothing.
pub fn prompt_question(
    vault: &Vault,
    allow_statuses: &[QuestionStatus],
    label: &str,
) -> Result<String> {
    let all = vault.list_questions()?;
    let eligible: Vec<_> = all
        .into_iter()
        .filter(|q| allow_statuses.contains(&q.status))
        .collect();
    if eligible.is_empty() {
        return Err(anyhow!(
            "no questions match the allowed statuses for this verb",
        ));
    }
    let labels: Vec<String> = eligible
        .iter()
        .map(|q| {
            let text = if q.question_text.is_empty() {
                "(no H1)".to_owned()
            } else {
                q.question_text.clone()
            };
            format!(
                "{} \u{2014} {text} ({}/{})",
                q.slug,
                q.domain.as_str(),
                q.status.as_str(),
            )
        })
        .collect();
    let pick = Select::new(label, labels.clone()).prompt()?;
    let idx = labels
        .iter()
        .position(|l| l == &pick)
        .expect("picked label was in the offered list");
    Ok(eligible[idx].slug.clone())
}

/// Fuzzy-pick an existing stewardship by slug. Used by `cdno
/// stewardship show`, `cdno stewardship add-periodic`, and `cdno
/// track`. The label shows the variant and tracking count so the
/// user can disambiguate flat from expanded at the picker.
pub fn prompt_stewardship(vault: &Vault, today: chrono::NaiveDate) -> Result<String> {
    let summaries = vault.list_stewardships(today)?;
    if summaries.is_empty() {
        return Err(anyhow!(
            "no stewardships \u{2014} create one with `cdno stewardship create`",
        ));
    }
    let labels: Vec<String> = summaries
        .iter()
        .map(|s| {
            let badge = match s.staleness_days {
                Some(days) if s.tracking_count > 0 => {
                    format!("{} tracking, last {} days ago", s.tracking_count, days)
                }
                _ => "no tracking yet".to_owned(),
            };
            format!("{} ({badge})", s.slug)
        })
        .collect();
    let pick = Select::new("Stewardship", labels.clone()).prompt()?;
    let idx = labels
        .iter()
        .position(|l| l == &pick)
        .expect("picked label was in the offered list");
    Ok(summaries[idx].slug.clone())
}

/// Fuzzy-pick an expanded stewardship — the only kind that accepts
/// tracking notes. Used by `cdno track` for the `--stewardship`
/// field. Errors when no expanded stewardship exists.
pub fn prompt_expanded_stewardship(vault: &Vault, today: chrono::NaiveDate) -> Result<String> {
    use cdno_domain::StewardshipVariant;
    let summaries = vault.list_stewardships(today)?;
    let eligible: Vec<_> = summaries
        .into_iter()
        .filter(|s| s.variant == StewardshipVariant::Expanded)
        .collect();
    if eligible.is_empty() {
        return Err(anyhow!(
            "no expanded stewardships \u{2014} create one with `cdno stewardship create --tracking`",
        ));
    }
    let labels: Vec<String> = eligible
        .iter()
        .map(|s| {
            let badge = match s.staleness_days {
                Some(days) => format!("last tracking {days} days ago"),
                None => "no tracking yet".to_owned(),
            };
            format!("{} ({badge})", s.slug)
        })
        .collect();
    let pick = Select::new("Stewardship", labels.clone()).prompt()?;
    let idx = labels
        .iter()
        .position(|l| l == &pick)
        .expect("picked label was in the offered list");
    Ok(eligible[idx].slug.clone())
}

/// Pick a [`Recurrence`] for a periodic commitment. Limited to the
/// closed enum's canonical phrases; for `EveryNMonths` we offer 2 / 3
/// / 6 which cover the common cases (bi-monthly, quarterly,
/// semi-annual). The user can type a custom recurrence with the
/// `--every` flag if they need something else.
pub fn prompt_recurrence() -> Result<Recurrence> {
    let labels: Vec<String> = vec![
        Recurrence::Daily.to_string(),
        Recurrence::Weekly.to_string(),
        Recurrence::Monthly.to_string(),
        Recurrence::EveryNMonths(2).to_string(),
        Recurrence::EveryNMonths(3).to_string(),
        Recurrence::EveryNMonths(6).to_string(),
        Recurrence::Yearly.to_string(),
    ];
    let pick = Select::new("Frequency", labels.clone()).prompt()?;
    Ok(pick
        .parse()
        .expect("picked label parses back as Recurrence"))
}

/// Pick a life-domain [`Context`] from the enum's variants.
pub fn prompt_context() -> Result<Context> {
    let labels: Vec<&'static str> = Context::ALL.iter().map(|c| c.as_str()).collect();
    let pick = Select::new("Context", labels.clone()).prompt()?;
    let idx = labels
        .iter()
        .position(|l| l == &pick)
        .expect("picked label was in the offered list");
    Ok(Context::ALL[idx])
}
