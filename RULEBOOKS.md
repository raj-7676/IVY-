# Ivy rulebooks

Every deterministic text rule Ivy runs lives in `src-tauri/src/rulebooks/`, one file per book.
They replaced the old 60-rule suite in `cleanup.rs` on 2026-10-02. About a dozen of those old rules
changed words the speaker actually said. For example:
- "you **to**" → "you **too**"
- "**use to**" → "used to"
- "the trial **period** ended" → "trial. ended"
- "I think **so**" → "I think"
- "Ram" → "RAM"

The speech model (Ivy lite: Qwen3-ASR-1.7B fine-tuned to hear and clean in one pass, self-corrections applied)
decides what was **meant**. The rulebooks do only two jobs:
- refuse output that was never said (book 1);
- format what was said, in the chosen tone.

They never guess meaning.

Test the books on their own (no app build needed):
`rustc --edition 2021 --test src-tauri/src/rulebooks/mod.rs -o rb.exe && rb.exe`

## The seven laws (every book obeys them; the tests check them)

1. **Whole words only.** A rule matches whole tokens, never letters inside a word.
2. **Never trade a spoken word for a different word.** Only these changes are allowed:
   - turning explicit spoken commands into symbols (books 3 and 5);
   - writing number words as digits, and big round amounts with the scale word ("18 lakhs") (book 4);
   - casing (book 6);
   - the tone's word swaps from fixed lists: Standard and Professional only (book 7);
   - adding missing apostrophes (book 8).
3. **When in doubt, leave it as spoken.** A missed format is cheap; a changed word is a hallucination.
4. **Idempotent.** Running a book twice gives the same text as running it once.
5. **Fixed order.**
   - Audio → Hallucinations A → lite model → Hallucinations B → books 3 → 7 → 4 → 5 → 6 → 8.
   - The model removes fillers and stutters itself, so there is no book 2 any more.
6. **Fast and dependency-free.** The books use only the standard library and stay far under 1 ms per dictation.
7. **Every rule has a "does" example and a "must not touch" example** in its book's tests.

---

## Book 1 — HALLUCINATIONS (outranks every other book)

**Definition.** A hallucination is any word in the output with no matching speech in the audio. It is
the worst failure a dictation app can have. A mishearing looks like a typo; a hallucination looks like
the user wrote something they never said.

### What we know (research)

| Finding | Source |
|---|---|
| On pure non-speech audio, Whisper hallucinated in **40%** of clips. Two-thirds of those were a small set of recurring phrases: "Thank you." (25%) and "Thanks for watching" (10%). | Barański et al., *Investigation of Whisper ASR Hallucinations Induced by Non-Speech Audio*, ICASSP 2025 — https://arxiv.org/abs/2501.11378 |
| **Voice-activity detection before recognition** (Silero VAD) cut those hallucinations to **0.2%**. Adding **loop removal + the bag of hallucinations** afterwards brought WER on noisy audio from 104% to 6.5%. | same paper |
| Very short (about 1 s) and very long (30 s) segments hallucinate the most. Beam size 1 and Whisper's built-in silence settings help only a little. | same paper |
| Hallucinations happen more for speakers with **long pauses** (non-vocal time). About 40% of hallucinations were harmful (invented violence, false associations, false authority). | Koenecke et al., *Careless Whisper*, ACM FAccT 2024 — https://arxiv.org/abs/2402.08021 |
| Whisper's no-speech threshold alone "is insufficient to substantially mitigate non-speech hallucinations". | *Reducing Hallucinated Transcripts in Whisper via Hallucination Space Projection*, 2026 — https://arxiv.org/abs/2609.04561 |
| LLMs used for ASR correction hallucinate by "modifying correct text". The fix is to verify every change against what was heard, not to trust the model. | Fang et al., *Fewer Hallucinations, More Verification*, 2025 — https://arxiv.org/abs/2505.24347 |

### What to follow

**Stage A — on the audio, before recognition** (`hallucinations::prepare_audio`)
- **A1 Silence in, nothing out.** Under 0.25 s of voiced audio → no transcription at all.
- **A2 Trim the silent edges.** Keep 0.3 s of margin around the speech.
- **A3 Shorten long pauses.** Pauses over 1.5 s become 0.6 s, because pauses are where hallucinations grow.
- The voice threshold is only 6 dB above the room's own noise floor, so a quiet word is never cut.

**Stage B — on the recognizer's text** (`hallucinations::clean_asr_text`)
- **B1 Bag of hallucinations, certain.** Outro, subscribe and caption-credit sentences are removed wherever they appear: "thanks for watching", "please subscribe", "subtitles by …", "amara.org". Nobody dictates these; they leak from YouTube captions in Whisper's training data.
- **B2 Bag of hallucinations, ambiguous.** "Thank you.", "Bye.", "You", "Okay." are removed only when they are the *entire* output **and** under 0.8 s of voice was heard. A real "Thank you." has real voice behind it.
- **B3 Loops.** A 2–8 word phrase repeated 3+ times in a row is kept once. (The decoder already stops token loops at 5 repeats.)
- **B4 Impossible speaking rate.** More than 7 words per voiced second cannot be speech. Trailing sentences are dropped until the rate is plausible, because hallucinations attach at the end, in the pauses.

### What to know (never "fix" into a hallucination)
- Never fill a gap. If audio is unclear, write what was heard or nothing, never a guess.
- Never complete a sentence the speaker abandoned.
- Never translate, summarize or answer during dictation.
- A word repeated on purpose ("no, no, no") is not a loop.
- Training data for Ivy's own models follows the same idea: a voice clip is only used when an independent recognizer hears the same words (`gen_clone.py`, Whisper check, WER ≤ 0.3).

### Upgrade path (measured best, not built yet)
- **Silero VAD** instead of the energy detector. It is the method in the ICASSP 2025 result; it's ONNX and Ivy already ships onnxruntime.
- **No-speech probability from Whisper's first decoder step**, combined with the VAD and not used alone.

---

## Book 3 — Spoken commands (`commands.rs`)
- **C1** "new line", "new paragraph" (not after "a"/"the": "a new line manager" stays).
- **C2** Punctuation:
  - "period"/"full stop" only at the end or before a capital, never after a noun-maker ("trial period", "a period").
  - Comma, colon, semicolon, question/exclamation mark never after a determiner ("a comma").
  - "colon surgery" stays.
- **C3** Open/close quote, paren(thesis), bracket, (curly) brace.
- **C4** "bullet point X" at a line/sentence start; "number one:" → "1." list items.
- **C5** Emoji only when the word "emoji" is said ("thumbs up emoji" → 👍; "draw a smiley face" stays).
- **C6** "hashtag X" → #X; "in backticks X" → \`X\`.

## Book 4 — Numbers: digits (`numbers.rs`)
- **N1** Dates: "March twenty first" / "the twenty first of March" → "March 21st". Lowercase "may"/"march" count only after in/on/by/next/last …, so "you may first check" stays.
- **N2** Years: "twenty twenty six" → 2026 (1950–2099).
- **N3** Times: "seven thirty pm" → 7:30 PM; "5 p.m."/"5pm"/"7.30 pm" → 5 PM / 7:30 PM.
- **N4** Number words → digits:
  - "two" → 2, "fifteen hundred" → 1500, "a hundred and fifty" → 150, "one point five" → 1.5.
  - 3+ single digits → one string ("six three seven nine" → 6379).
- **Stay words:**
  - "one"/"zero" alone (no one, the one, one of them) unless a unit or time follows ("one hour" → "1 hour");
  - "the two of us", "a day or two";
  - ordinals outside dates ("for the first time").
- **N5** After a number: % ($, ₹, €, ¥), km, kg, cm, mm, mg, mL, L, m, g, KB, MB, GB, TB, mph, km/h, °C, °F, °. "pounds" stays (weight or money?).
- **N6** Math only between numbers: "5 plus 10 equals 15" → "5 + 10 = 15". "2 times a day" stays.
- **N7** "plus 91 98765 …" → "+91 98765 …".
- **N8** Big round amounts get the scale word (Yash: long zeros confuse people):
  - Indian grouping or ₹ → lakhs/crores: "18,00,000" → "18 lakhs", "₹1,15,000" → "₹1.15 lakhs", "2,50,00,000" → "2.5 crores".
  - Western grouping or $ € £ → million/billion: "$2,000,000" → "$2 million".
  - Only from 1 lakh / 1 million up, only when 2 decimals say it exactly ("1,15,437" and "68,990" stay), and only for numbers with commas or a currency sign (never phone numbers or IDs).

## Book 5 — Tech (`tech.rs`)
- **T1** Links: "https colon slash slash", "www dot", "x dot com", "slash path".
- **T2** Emails only when the domain was spoken with "dot" + a known ending. "Farhan at accounts" and "Ravi at the office" stay.
- **T3** File extensions ("config dot ts" → config.ts; "the dot ts file" → "the .ts file").
- **T4** Code casing commands (camel, snake, kebab, pascal, screaming snake).
- **T5** Operators said by name (plus equals, double equals, fat arrow …).
- **T6** Shortcuts ("control c" → Ctrl+C).
- **T7** "localhost colon 3000".
- **T8** "x squared" → x².

## Book 6 — Names and capitals (`names.rs`)
Only the **case** changes, never the word.
- Brands that are never ordinary words: GitHub, JavaScript, Node.js. Not react, slack, notion, rust or excel.
- Acronyms (API, PDF …). Not "ram" (a name) and not "ml" (millilitres).
- Days, months ("may"/"march" need a date word), holidays, nationalities/languages, time zones (IST, EST).
- Sentence starts and "I".

## Book 7 — Tone (`tone.rs`)
Each tone is a step up from the one before (2026-10-06; research: rule-based formality keeps meaning best, GYAFC 2018).
- **Casual: nothing changes.** The speaker's own wording ("gonna", "like", "yeah") stays.
- **Standard:**
  - slang → full words (gonna, wanna, gotta, kinda, cuz …);
  - ", like," and ", you know," are dropped;
  - "off of" → "off".
- **Professional:** Standard, plus
  - contractions written out (don't → do not, can't → cannot, it's been → it has been, I'd been → I had been);
  - chat words → formal (yeah → yes, nope → no, btw → by the way, asap → as soon as possible, thanks → thank you);
  - an opening "Honestly, / Basically, / Literally, / Actually," is dropped;
  - no exclamation marks ("!" → ".").
- **Removed for good:** grammar "fixes", homophone guesses, sound-alike guesses, and phrase rewrites ("in order to" → "to").

## Book 8 — Typography (`typography.rs`)
- Missing apostrophes (dont → don't, im → I'm; not lets, id, wont, ill, its).
- "could of" → "could have".
- "e g" → e.g., "i e" → i.e., "et cetera" → etc.
- A final dangling and/but/or/because is dropped (not "so": "I think so").
- Spacing and punctuation hygiene ("…" kept, " .ts" kept).

## Touch Up (`spellcheck.rs`, not a book: only when the user clicks it)
- Fixes misspelled words in a dictation that was already pasted ("recieve" → "receive"). Never rephrases.
- Checks only plain lowercase words of 3+ letters: names, acronyms, numbers, emails, file names and contractions are never touched.
- Leaves known words and a keep list (Indian English, Hinglish, chat words: lakh, chai, yaar, haan …) alone.
- A fix must be a common word, 1 edit away (2 for words of 6+ letters); the most frequent candidate wins.
- Word list: SymSpell's English frequency dictionary (80,000 words, MIT).
