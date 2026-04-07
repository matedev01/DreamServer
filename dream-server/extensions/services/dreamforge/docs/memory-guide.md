# Memory System Guide

DreamForge has a persistent memory system that stores context across sessions. Memories help the agent remember who you are, how you work, what your project needs, and where to find things -- so you don't have to repeat yourself every conversation.

This guide covers how memories are stored, retrieved, and managed. For full API details, see [api-reference.md](api-reference.md).

---

## What Is Memory?

Every time you start a new session with DreamForge, the agent has no inherent knowledge of your past conversations. The memory system fills that gap. Memories are small, persistent files that carry forward the context the agent needs to work effectively with you over time.

Think of them as notes the agent keeps about you and your work. Some you write yourself; others are extracted automatically from conversations.

---

## Memory Types

There are four memory types, each serving a different purpose and receiving a different priority during retrieval.

| Type | Purpose | Examples |
|------|---------|---------|
| **user** | Who you are -- role, expertise, preferences | "Senior backend engineer", "Prefers Python over Go" |
| **feedback** | Approach guidance -- what to do and what to avoid | "Don't mock the database in tests", "Keep PRs small" |
| **project** | Ongoing work context | "Auth migration due March 15", "Merge freeze Thursday" |
| **reference** | Pointers to external resources | "Bugs tracked in Linear project INGEST", "Grafana board at grafana.internal/d/api-latency" |

Choosing the right type matters. Each type gets a different relevance boost during retrieval (see [How Retrieval Works](#how-retrieval-works) below), so putting a project deadline in a `user` memory means it will be scored differently than it should be.

---

## How Memories Are Stored

Each memory is a markdown file with YAML frontmatter, stored in `{DATA_DIR}/memory/`.

```markdown
---
id: a1b2c3d4
type: user
title: User is a data scientist
description: Prefers pandas and matplotlib for data analysis
created: 2025-01-15T10:00:00Z
updated: 2025-01-15T10:00:00Z
---

User works as a data scientist. Prefers pandas over SQL for data
manipulation. Uses matplotlib and seaborn for visualization.
```

**File naming convention:** `{slug}_{id}.md`

For example: `user_preferences_a1b2c3d4.md`

The slug is derived from the title, and the ID is a unique identifier generated at creation time.

### The Index File

An index file (`MEMORY.md`) is auto-rebuilt whenever memories change. It contains one-line summaries of each memory for quick scanning. You generally do not need to edit this file by hand -- it is regenerated automatically.

---

## Creating Memories

There are three ways to create memories.

### From the UI

Open the Memory panel (`Ctrl+Shift+M`) and click **New Memory**. Choose a type, add a title, and write the content.

### From the API

```bash
curl -X POST http://localhost:3010/api/memory \
  -H "Authorization: Bearer <api_key>" \
  -H "Content-Type: application/json" \
  -d '{
    "type": "user",
    "title": "User is a data scientist",
    "content": "Prefers pandas and matplotlib.",
    "description": "User role and tool preferences"
  }'
```

See [api-reference.md](api-reference.md) for the full request and response schema.

### Automatic Extraction

After each conversation, DreamForge can automatically extract and save relevant memories from the conversation. This happens post-session and captures things like user preferences, project context, and feedback that emerged during the chat.

You do not need to do anything to enable this -- it runs automatically. If the agent picks up something you said about your workflow or preferences, it may appear as a new memory the next time you check the Memory panel.

---

## Editing and Deleting Memories

### From the UI

Open the Memory panel, click on any memory, and edit or delete it directly.

### From the API

**Update a memory:**

```bash
curl -X PUT http://localhost:3010/api/memory/{entry_id} \
  -H "Authorization: Bearer <api_key>" \
  -H "Content-Type: application/json" \
  -d '{"title": "Updated title", "content": "Updated content"}'
```

**Delete a memory:**

```bash
curl -X DELETE http://localhost:3010/api/memory/{entry_id} \
  -H "Authorization: Bearer <api_key>"
```

The index file is rebuilt automatically after any create, update, or delete operation.

---

## How Retrieval Works

When the agent needs context, it searches memories using keyword overlap. Here is the process:

1. **Query extraction** -- Terms are extracted from the query, filtered for stopwords, and reduced to alphanumeric tokens.
2. **Scoring** -- Each memory is scored against the query using its title, description, and the first 500 characters of content.
3. **Type-based boost** -- Scores are multiplied by a factor based on memory type:

   | Type | Boost |
   |------|-------|
   | feedback | 1.2x |
   | project | 1.1x |
   | user | 1.0x |
   | reference | 0.8x |

4. **Result selection** -- The top 5 results are returned, each capped at 4,096 characters.

This means **feedback memories are slightly prioritized** because they guide the agent's approach, while **reference memories are slightly deprioritized** because they are pointers to external resources rather than direct answers.

---

## Limits

| Limit | Value |
|-------|-------|
| Max memory files scanned | 200 |
| Max content per memory | 50,000 characters |
| Max content per index entry | 4,096 characters |
| Index file (`MEMORY.md`) max | 200 lines / 25 KB |

If you exceed 200 memory files, older or lower-priority files may not be scanned. Consider cleaning up stale memories periodically.

---

## Best Practices

**Be specific in titles.** A title like "User prefers pytest over unittest" is far more useful for retrieval than "Testing preferences." The title is one of the primary fields used for relevance matching.

**Keep descriptions short.** The description field is used during scoring, so a concise, keyword-rich description improves retrieval accuracy. One or two sentences is ideal.

**Use the right type.** Do not put project deadlines in user memories or feedback in reference memories. The type determines the retrieval boost, and a misclassified memory may surface at the wrong time or not at all.

**Clean up stale memories.** Project memories especially can become outdated fast. A memory about a deadline that passed two months ago is just noise. Delete or update it.

**Let feedback accumulate naturally.** The most valuable feedback memories come from corrections during real work. When the agent does something you do not like and you correct it, that correction is a strong candidate for a feedback memory -- either created by you or extracted automatically.

---

## Further Reading

- [api-reference.md](api-reference.md) -- Full API request/response schemas for all memory endpoints
- [configuration-reference.md](configuration-reference.md) -- Settings that control memory behavior, including extraction and retrieval options
