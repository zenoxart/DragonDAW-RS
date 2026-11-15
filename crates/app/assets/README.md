# App assets

`splash.gif` – the animated Dragon DAW logo generated with **Higgsfield AI** (GPT Image 2.6, animated GIF
output). Put the file here (or in `assets/` next to the executable) and the splash screen plays it on start.
Without the file, the app draws the animated logo itself (`src/splash.rs`).

`levelup.gif` – the level-up animation generated with **Higgsfield AI** (GPT Image 2.6, animated GIF, job
`0f3064cf`). Put the file here (or in `assets/` next to the executable) and quest celebrations play it in the
card's emblem. Without the file (or with *Reduce motion*), the card draws its emblem itself
(`src/views/quests.rs`).
