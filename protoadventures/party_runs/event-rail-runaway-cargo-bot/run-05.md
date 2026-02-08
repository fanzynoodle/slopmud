---
adventure_id: "event-rail-runaway-cargo-bot"
run_id: "run-05"
party_size: 1
party_levels: [5]
party_classes: ["paladin"]
party_tags: ["solo", "body-block", "late-stop", "learning-run"]
expected_duration_min: 30
---

# Party Run: event-rail-runaway-cargo-bot / run-05

## Party

- size: 1
- levels: 5
- classes: paladin
- tags: solo, body-block, late-stop, learning-run

## What They Did (Timeline)

1. hook / acceptance
   - Triggered the event and took it as a personal vow: "I will stop it, even alone."
2. travel / navigation decisions
   - Ran the main line first and only noticed the lever platform after passing the yard hazards once.
3. key fights / obstacles
   - Without slows, the paladin couldn't meaningfully change the bot's speed until the final stop point.
   - Geometry mistakes were punishing solo; one crane clip almost ended the run.
4. setpiece / spike
   - At the stop point, they body-blocked during the ram tell and survived by playing defensively rather than trying to race damage.
5. secret / shortcut (or why they missed it)
   - Missed the lever route on the first pass because it wasn't signposted early enough; felt like required knowledge for solo.
6. resolution / rewards
   - Stopped the bot late; the terminal took damage but didn't fully lock down. Base payout, plus a strong "you can succeed but not clean" lesson.

## Spotlight Moments (By Class)

- paladin: committed to the body-block play and timed it to the ram tell.
- paladin: learned lane discipline the hard way; switched to safe lanes and finished with low HP.
- paladin: chose to ignore bystanders to prevent a total terminal failure (explicit moral cost).

## Friction / Missing Content

- Solo guidance needs to exist (lever route hint, body-block timing hint, or a weaker bot for party_size=1).
- Lever signage must appear before the yard, not after players have already committed.
- Terminal damage state should be clear: success-but-degraded needs visible consequences and messaging.

## Extracted TODOs

- TODO: Add a dispatcher line for solo/small parties: "lever platform is your best slowdown."
- TODO: Add early signage for the lever route (before the first hazard segment).
- TODO: Add a clear terminal damage readout and what it changes (travel lockout, vendor closure, etc).

