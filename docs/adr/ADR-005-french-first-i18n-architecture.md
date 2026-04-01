# ADR-005: French-First i18n Architecture

**Date:** 2026-03-31
**Status:** Accepted
**Deciders:** Product & Architecture Division

---

## Context

Maintafox targets industrial maintenance teams where French is the primary operational
language at launch. English is required from day one as a secondary locale for
international deployments and for the engineering and support workflow. Future locale
expansion (Arabic, Portuguese, Spanish) is a realistic mid-term requirement.

The product contains dense technical vocabulary: failure modes, maintenance states,
equipment classifications, permit types, inspection checkpoints. Translation quality
matters for safety and regulatory compliance, not only UX polish.

The question is whether to treat i18n as a post-launch concern or as a day-one
architectural constraint.

## Decision

We will implement **French-first i18n as a day-one architectural constraint**. French is
the default locale. Every user-visible string in the application is a translation key.
No French or English text may appear as a literal string in React component files. The
i18n resource model uses namespace files scoped to module boundaries. Missing translation
keys fail the build in production mode.

## Rationale

- Adding i18n to an existing codebase that was built with hardcoded strings is a full
  rewrite of every user-facing file — doing it correctly from sprint 1 costs less
  overall than retrofitting it after 50 modules are built
- French-as-default forces correct i18n behavior from the first line of UI code; English
  becomes an equally serviced locale rather than the implied fallback
- Namespace files scoped to modules (e.g., `workOrder.json`, `equipment.json`) allow
  each module sprint to own its translations without polluting a single global file
- A failing build on missing keys prevents the classic "key shown instead of translation"
  bug from reaching the supervisor acceptance stage
- The maintenance engineering domain requires precise terminology — translation governance
  belongs in the architecture, not as a manual QA step

## Alternatives Considered

| Alternative | Reason Not Chosen |
|---|---|
| Hardcode French, add i18n later | Creates a rewrite project mid-delivery; all component files must be touched; tested-and-accepted behavior breaks |
| English-first with French overlay | English becomes implicitly "correct"; French becomes a translation of a translation; domain vocabulary lose precision |
| Runtime locale loading from VPS | Adds VPS connectivity dependency for UI rendering; breaks offline operation |
| Single large `translations.json` | Untraceable key ownership; merge conflicts on every concurrent sprint; no per-module governance |

## Consequences

**Positive outcomes:**
- French and English are production-quality peers from the first release
- Module teams own their namespace files — no centralized translation bottleneck
- Future locale addition follows a known pattern: add namespace file, nothing else changes

**Trade-offs and costs:**
- Every sprint that adds a UI element must also add the translation key to both
  `fr/` and `en/` namespace files — adds 5-10 minutes per sprint
- The supervisor must review French copy on every acceptance test; errors in French text
  require a fix sprint, not just a comment

## Linked Resources

- PRD Section 2.2 — Strategic Objective 7: Multilingual by design
- PRD Section 6.26 — Configuration Engine (tenant terminology customization)
- `docs/CODING_STANDARDS_FRONTEND.md` — i18n Rule (Section 7)
