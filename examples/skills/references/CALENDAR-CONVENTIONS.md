# Calendar Conventions

Skills that surface the day's schedule read the user's calendars through the calendar MCP server. Which calendars exist, and any naming conventions on them, are **user-specific configuration** — the notes below describe the *mechanism* to support, not a fixed setup. Adapt to whatever the user actually has.

## Multiple calendars by context

A user may keep several calendars separated by context — for example a work calendar, a personal calendar, and a shared or family calendar. When summarising the day, group or label events by context where it helps the user orient. Purely informational calendars (holidays, birthdays) can be shown but don't need special handling — skip them unless asked.

## Ownership prefixes on shared calendars

Some users mark events on a shared calendar with a short prefix to indicate whose event it is — typically a per-person initial followed by a colon, with no prefix meaning a shared event:

| Prefix | Meaning | Example |
|--------|---------|---------|
| an initial + `:` | Belongs to one person | `X: Appointment` |
| *(no prefix)* | Shared event | `Shared dinner` |

If the user's calendar uses such a convention, interpret the prefix when describing events ("your appointment" vs "someone else's" vs "a shared event"). If there's no such convention, treat all events plainly. Never assume a specific scheme — follow what the user's calendars actually do, and ask if it's unclear.
