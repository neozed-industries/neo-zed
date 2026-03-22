---
title: Privacy for Business - Zed Business
description: How Zed Business handles data privacy across your organization, including enforced protections for prompts and training data.
---

# Privacy for Business

Zed Business removes the per-member data-sharing options that Free and Pro expose. Administrators control these settings for the whole organization; individual members can't opt in or out.

<!-- TODO: confirm with Cloud team whether these protections are on by default at org creation or require admin configuration before launch -->

## What's enforced

For all members of a Zed Business organization:

- **No prompt sharing:** Conversations and prompts are never shared with Zed. Members can't opt into [AI feedback via ratings](../ai/ai-improvement.md#ai-feedback-with-ratings).
- **No training data sharing:** Code context is never shared with Zed for [Edit Prediction model training](../ai/ai-improvement.md#edit-predictions). Members can't opt in individually.

These protections are enforced server-side and apply to all org members.

## How individual plans differ

On Free and Pro, data sharing is opt-in:

- Members can rate AI responses, which shares that conversation with Zed.
- Members can opt into Edit Prediction training data collection for open source projects.

Neither option is available to Zed Business members.

## What data still leaves the organization

These controls cover what Zed stores and trains on. They don't change how AI inference works: when members use Zed's hosted models, prompts and code context are still sent to the relevant provider (Anthropic, OpenAI, Google, etc.) to generate responses. Zed maintains zero-data retention agreements with these providers. See [AI Improvement](../ai/ai-improvement.md#data-retention-and-training) for details.

[Bring-your-own-key](../ai/llm-providers.md) and [external agents](../ai/external-agents.md) are subject to each provider's own terms; Zed has no visibility into how they handle data.

## Additional admin controls

Administrators have additional options in [Admin Controls](./admin-controls.md):

- Disable Zed-hosted models entirely, so no prompts reach Zed's infrastructure
- Disable Edit Predictions org-wide
- Disable real-time collaboration

See [Admin Controls](./admin-controls.md) for the full list.
