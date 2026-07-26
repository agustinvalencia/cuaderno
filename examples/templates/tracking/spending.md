---
type: tracking
stewardship: {{stewardship}}
activity: spending
date: {{date}}
# One record per line item. `category` is what the series split on, so the
# same category can recur within an entry and still land in one series;
# `amount` is a total, so it sums. Records live in frontmatter rather than a
# table because a table column cannot be split by a category.
detail:
  - { category: groceries, amount: 0.00 }
  - { category: transport, amount: 0.00 }
---

# Spending — {{date_long}}

## Notes
{{content}}
