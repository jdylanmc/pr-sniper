# Logic Proof of Concept

Use this presentation shape within [poc](SKILL.md)'s isolation, execution, and findings boundaries. Work in the agreed scratch environment, not the product checkout.

Build a single, self-contained HTML file (a **shareable demo**) for anyone to drive a state model with buttons. Use it for **business logic, state transitions, or data shape** that looks reasonable on paper but feels wrong in real cases.

One file, nothing to install: hand it to a non-developer (a designer, a PM, a domain expert) to feel the model themselves. Speak their language, not the code's.

## When this is the right shape

- "I'm not sure if this state machine handles the edge case where X then Y."
- "Does this data model actually let me represent the case where..."
- "I want to feel out what the API should look like before writing it."
- Anything where someone wants to **press buttons and watch state change**.

If the question is "what should this look like," this is the wrong branch. Use [UI.md](UI.md).

## Process

### 1. State the question

Before coding, state the model and question in one paragraph at the demo's top: a visible intro, not just a comment. Make the question checkable now or when the user returns AFK; answering the wrong question wastes the prototype.

### 2. Isolate the logic in a portable module

Put the logic answering the question in one `<script>` block as a small, pure module. Separate investigated behavior from display so observations are easy to inspect. Both module and page remain experimental.

Choose the shape for the question:

- **A pure reducer**: `(state, action) => state`. Good when actions are discrete events and state is a single value.
- **A state machine**: explicit states and transitions. Good when "which actions are even legal right now" is part of the question.
- **A small set of pure functions** over a plain data type. Good when there's no implicit current state, just transformations.
- **A class or module with a clear method surface** when the logic genuinely owns ongoing internal state.

Choose what fits the question, *not* what's easiest to wire to a page. Keep it pure: no DOM, no `document`, no button handlers reaching inside. The page calls it; nothing flows back. Findings can inform later implementation; do not copy the experimental module into production.

### 3. Build the shareable HTML file

One plain HTML/CSS/JS file: no framework, bundler, or server. Inline everything so anyone can double-click to open it, even after emailing it around.

Write for a non-developer. Use **domain language**, not code, for every label: buttons and state read like the business, not the reducer. Explain what's happening in plain words.

Use a clean hierarchy, top to bottom:

1. **Title and one-line explanation** of what the demo explores (the question from step 1).
2. **Current state**: full relevant state in a readable panel (labelled fields, not raw JSON), re-rendered after every click to show changes. Call out what just changed where it helps a non-developer follow.
3. **Free-play buttons**: one per action, always available for exploring in any order. Each click dispatches its action and re-renders state.
4. **Guided walkthroughs**: **scenarios**, one per tab. Each tab has a short plain-language description (setup and what to watch for), then the ordered **buttons to press**. Each step is a real button that performs its action and advances to the next step. Starting a walkthrough resets to a known initial state for repeatable runs.

Choose scenarios covering awkward cases hard to reason about on paper: the happy path, a tricky edge case, an attempt at something that should be illegal.

Keep it beautiful but restrained: clean typography, generous spacing, one accent colour. No animations or gimmicks; nothing competing with state and buttons.

### 4. Hand it over

Send or open the file for them. They'll explore walkthroughs and free-play when available. Listen for "wait, that shouldn't be possible" or "huh, I assumed X would be different": bugs in the _idea_ are the point. Add actions or scenarios they request; prototypes evolve.

### 5. Capture the answer and the prototype

Run guided cases and record observed state transitions. Include the answer, human feedback, and rerunnable HTML file in the [poc](SKILL.md) findings packet. Leave product modules unchanged; do not commit or publish the demo as part of the experiment.

## Anti-patterns

- **Don't build a production test suite.** Small assertions or checks establishing the answer help; unrelated coverage does not.
- **Don't wire it to the real database.** Use in-memory state unless the question is specifically about persistence.
- **Don't generalise.** No "what if we wanted to support X later." The prototype answers one question.
- **Don't blur the logic and the page together.** Keep the page a thin shell over a pure module so rendering does not obscure tested behavior.
- **Don't reach for a framework, bundler, or server.** One file the recipient double-clicks; a React app or dev server defeats "shareable".
- **Don't ship the experiment into production.** Demo shell and logic module answer a question, not product requirements.
