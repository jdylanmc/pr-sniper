# UI Proof of Concept

Use this presentation shape within [poc](SKILL.md)'s isolation, execution, and findings boundaries. All page and route changes stay in the agreed scratch copy or isolated worktree, never the working product checkout. Use synthetic fixtures and fake authentication, not real data or secrets.

Generate **several radically different UI variations** on one route, switchable from a floating bottom bar. The user flips between variants in the browser, picks one (or bits from each), then discards the rest.

For logic/state questions rather than appearance, use [LOGIC.md](LOGIC.md), not this branch.

## When this is the right shape

- "What should this page look like?"
- "I want to see a few options for this dashboard before committing."
- "Try a different layout for the settings screen."
- Any time the user would otherwise spend a day picking between three vague mockups in their head.

## Two sub-shapes: strongly prefer sub-shape A

The app's surrounding header, sidebar, and realistic synthetic-data density make UI easier to judge. Default to sub-shape A when an isolated existing-page copy can safely host variants. Use sub-shape B when that context is unavailable or unnecessary.

### Sub-shape A: adjustment to an existing page (preferred)

Use the existing route in the isolated copy. Render variants **on the same route**, gated by a `?variant=` URL search param. Preserve layout and parameter shape, but replace live data fetching and authentication with local fixtures or stubs. Only rendering swaps.

Something without a page that *would naturally live inside one* (a new section of the dashboard, a new card on the settings screen, a new step in an existing flow) is still sub-shape A. Mount variants inside the host page.

### Sub-shape B: a new page (last resort)

Use only when the prototype genuinely has no existing host page (e.g. an entirely new top-level surface, or a flow that can't be embedded anywhere sensible).

Create a **throwaway route** using the project's routing convention, not a new top-level structure. Name it obviously as a prototype (e.g. include `prototype` in the path or filename). Same `?variant=` pattern.

Before choosing sub-shape B, check: is there really no existing page to embed it in? Empty routes hide design problems populated ones expose.

Use the identical floating bottom bar in both sub-shapes.

## Process

### 1. State the question and pick N

Default to **3 variants**; cap at 5. Beyond that, radical differences become noise.

Write a one-line plan in the prototype's location or a top-of-file comment:

> "Three variants of the settings page, switchable via `?variant=`, on the existing `/settings` route."

Do this whether the user is present to push back or not.

### 2. Generate radically different variants

Draft each variant, respecting:

- The page's purpose and accessible data.
- The project's component library / styling system (TailwindCSS, shadcn, MUI, plain CSS, whatever).
- A clear exported component name, e.g. `VariantA`, `VariantB`, `VariantC`.

Variants must be **structurally different**: layout, information hierarchy, primary affordance—not just colours. Three slightly-tweaked card grids aren't a UI prototype. If two drafts are too similar, redo one with explicit "do not use a card grid" guidance.

### 3. Wire them together

Create one switcher component on the route:

```tsx
// pseudo-code, adapt to the project's framework
const variant = searchParams.get('variant') ?? 'A';
return (
  <>
    {variant === 'A' && <VariantA {...data} />}
    {variant === 'B' && <VariantB {...data} />}
    {variant === 'C' && <VariantC {...data} />}
    <PrototypeSwitcher variants={['A','B','C']} current={variant} />
  </>
);
```

For sub-shape A (existing page): keep fixture loading above the switcher; only the rendered subtree changes per variant. Do not reconnect live data fetching.

For sub-shape B (new page): the throwaway route under `/prototype/<name>` mounts the same switcher.

### 4. Build the floating switcher

A small fixed-position bar at the screen's bottom-centre with three pieces:

- **Left arrow**: cycles to the previous variant (wraps around).
- **Variant label**: shows the current key and exported name, if any. e.g. `B (Sidebar layout)`.
- **Right arrow**: cycles forward (wraps around).

Behaviour:

- Arrows update the URL search param via the framework's router (e.g. `router.replace` on Next, `navigate` on React Router, etc), making variants shareable and reload-stable.
- Keyboard: `←` and `→` also cycle. Don't intercept arrow keys when an `<input>`, `<textarea>`, or `[contenteditable]` is focused.
- Visually distinct from the page (e.g. high-contrast pill, subtle shadow), clearly separate from the design being evaluated.
- Hidden in production builds: gate on `process.env.NODE_ENV !== 'production'` or equivalent so a stray prototype merge can't ship the bar to users.

Use one shared switcher component for both sub-shapes, located wherever shared UI lives in the project.

### 5. Hand it over

Surface the URL and `?variant=` keys. The user can flip through later. Feedback like **"I want the header from B with the sidebar from C"** reveals the actual design they want.

### 6. Capture the answer and clean up

Run variants, exercise the switcher, and record observed behavior. Capture supplied human preference and reasoning; otherwise mark that judgment pending. Return all variants and findings as [poc](SKILL.md) describes, including the local run command and any remaining server.

Keep the experiment in scratch. Do not promote a winning variant, commit a branch, or alter the product page. Separately authorized implementation can use findings without treating experimental code as production-ready.

## Anti-patterns

- **Variants that differ only in colour or copy.** That's a tweak, not a prototype. Real variants disagree about structure.
- **Sharing too much code between variants.** A shared `<Header>` is fine; a shared `<Layout>` defeats the point. Each variant should be free to discard the layout.
- **Wiring variants to real mutations.** Read-only prototypes are fine. Point mutations at a stub: the question is "what should this look like", not "does the backend work".
- **Promoting the prototype directly to production.** Variant code was written under experimental constraints. Return findings; product implementation is separate work.
