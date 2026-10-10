# Intent: joe-mode-cmux

Joe-mode-cmux is a human-activated, session-bound CMUX cockpit for Joe-mode. It preserves Joe-mode as the single repository controller and makes its Project Manager, Discovery, developer, and Shepherd work visible and directly interactive in one prescribed workspace.

Activation reconciles the existing Joe-mode controller and surviving legacy owners/jobs before creating anything. It uses the invoking CMUX workspace and exact surface identities, keeps the current session as Project Manager, gives Discovery and support distinct panes only when supported placement is verified, and stacks developer surfaces together as tabs.

Managed workers must launch through CMUX Maestro using the invoking session's verified Copilot account and an explicitly selected model. Missing or unavailable identity or launch settings fail before a terminal is created. Role icons, labels, workspace status, progress, and logs should make ownership and attention needs clear without implying authority or task success.

CMUX provides the visible cockpit, not unattended execution. Restored panes do not prove supervision, and this skill does not invent cron, heartbeats, scheduler behavior, merge authority, tracker approval, or human decisions. It remains active only for the current session, preserves focus by default, uses explicit handles, and stops dispatch when ownership or runtime identity cannot be verified.
