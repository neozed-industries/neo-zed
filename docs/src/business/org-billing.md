---
title: Organization Billing - Zed Business
description: How billing works for Zed Business organizations, including consolidated invoicing, AI spend limits, and per-member usage visibility.
---

# Organization Billing

<!-- TODO: verify org billing behavior with Cloud team before launch -->

Zed Business consolidates your team's costs. Seat licenses and AI usage for all members appear on one bill, with no separate invoices per member.

## Billing dashboard {#dashboard}

<!-- TODO: confirm exactly what billing information is shown in the dashboard before launch -->

Owners and admins can access billing information at [dashboard.zed.dev](https://dashboard.zed.dev). The dashboard shows:

- Current member count and seat charges
- AI usage and spend across the organization
- Per-member usage and spending visibility

## AI usage {#ai-usage}

AI usage across the organization is metered on a token basis at the same rates as individual Pro subscriptions. See [Plans & Pricing](../ai/plans-and-usage.md#usage) for rate details.

<!-- TODO: confirm org-wide spend limit behavior at launch (how limits are set, what happens when reached) -->

Administrators can set an org-wide AI spend limit from [Admin Controls](./admin-controls.md). Once the limit is reached, AI usage is paused until the next billing period.

## Payment and Invoices {#invoices}

Organization billing uses Stripe for payments, via credit card or other supported payment method.

<!-- TODO: confirm whether invoice-based billing is available at launch -->

Invoice history is accessible from the billing dashboard.

<!-- TODO: confirm whether owners/admins can update payment methods, addresses, and tax IDs self-serve at launch; if so, replace the below with instructions -->

For help updating payment methods, names, addresses, or tax IDs, email [billing-support@zed.dev](mailto:billing-support@zed.dev).

Changes to billing information affect future invoices only. Historical invoices cannot be modified.

## Sales tax {#sales-tax}

Zed partners with [Sphere](https://www.getsphere.com/) to calculate indirect tax rates based on your billing address. Tax appears as a separate line item on invoices. If you have a VAT/GST ID, add it during checkout.

Email [billing-support@zed.dev](mailto:billing-support@zed.dev) with any tax questions.
