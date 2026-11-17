# Motion, sound and haptics

## Motion tokens
Curves are cubic-bezier. "Reduce motion" (ACCESSIBILITY) swaps any movement for a 120 ms fade and turns off camera shake and HUD bob.

| Token | Duration | Curve | Use |
|---|---|---|---|
| `m-press` | 60 ms | (0.2, 0, 0, 1) | button press scale 0.97 |
| `m-hover` | 120 ms | (0.2, 0, 0, 1) | hover lift and colour change |
| `m-hit` | 120 in / 180 out | out-expo | hit marker |
| `m-event` | 160 ms in, hold 1.6 s, 200 ms out | (0.16, 1, 0.3, 1) | centre event lines |
| `m-banner` | 240 ms | (0.16, 1, 0.3, 1) + 1 overshoot 4 px | match banners |
| `m-screen` | 220 ms | (0.4, 0, 0.2, 1) | screen push: new screen slides 24 px from the right and fades in; old screen fades to 0 over 120 ms |
| `m-wipe` | 180 ms | (0.7, 0, 0.2, 1) | callout-strip wipe: tab first, slab reveals left → right 60 ms later, leader 60 ms after |
| `m-modal` | 180 ms | (0.2, 0, 0, 1) | modal scale 0.96 → 1 + scrim fade |
| `m-count` | 400 ms | linear digits | count-ups |
| `m-pulse` | 800 ms loop | sine | low health, timer < 10 s |

**Rule:** UI motion never blocks input. A tap during a transition goes to the new screen.

## Feedback per event

The UI, camera and sound side of [PRINCIPLES.md](../../../../docs/PRINCIPLES.md) §5.1 (G4, G5, G7, G8) and §5.3. Haptics apply to gamepad rumble now and phones later.
| Event | Visual | Camera | Audio | Haptic | Latency |
|---|---|---|---|---|---|
| Fire | muzzle flash (1 frame light + sprite), shell eject, tracer | recoil kick 0.6–1.2° pitch, recovers 120 ms; viewmodel kick | `wpn_fire` layered close/far + tail by room size | light tick | same frame as input |
| Hit | hit marker, enemy flinch, impact particles | — | `ui_hit` (tick); head: `ui_hit_head` | — | ≤ 1 frame after confirmation (predicted) |
| Kill | marker → ×, event line, kill feed | 60 ms micro-zoom 2 % | `ui_kill` sting | medium | — |
| Take damage | direction arc, red vignette flash 80 ms | shake 0.15 trauma | `pl_hurt` + muffled low-pass when HP < 30 % | heavy on big hits | — |
| Reload | ammo numeral greys out, then a 120 ms pop | viewmodel anim | 3-part reload foley | none | — |
| Destruction | chunks + dust + rubble decals | shake by distance (trauma 0.3 → 0 over 25 m) | `dst_concrete_*` with low-end boom | medium, ≤ 15 m | — |
| Score | score numeral count-up + `+points` event | — | `ui_score` rising blip | light | — |
| Collapse warning | hazard-striped icon + edge arrow | slow low rumble shake | `dst_creak_*` + deep groan, rising | low rumble | from first creak |
| UI button | `m-press` | — | `ui_tap` (−18 LUFS short) | light | ≤ 50 ms |

## Sound language
- **Engine:** kira ([decision index](../../../../docs/README.md) D12), wrapped by the `audio` crate.
- **Mix buses (kira tracks):** Master → {Music, SFX → {Weapons, Foley, Destruction, UI}, Voice}. Ducking: Voice ducks Music by 6 dB, a near explosion ducks everything except UI by 4 dB for 300 ms.
- **Loudness:** gameplay mix −16 LUFS integrated; −23 when "Night mode" is on; true peak −1 dBTP.
- **Formats:** SFX are Ogg Vorbis q4 mono 44.1 kHz, decoded on load (budget 30 MB on the Low tier); music is streamed Ogg q3 stereo.
- **Spatial:** distance attenuation and a low-pass by occlusion ray (1 ray per emitter per 100 ms). Headphone 3D sound (HRTF) is tested later behind the same wrapper.
- **UI sound family:** short, dry, mechanical clicks and synth blips (think site machinery and radios), all from one in-house synth patch so they feel like one set. Any voice lines are original writing.
