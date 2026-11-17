# input-actions: every device reaches gameplay through named actions defined in data (Unreal Enhanced Input) (2026-11-12)

**What is wrong.**
- `engine-platform` hard-codes one game's bindings in the window layer: `KeyCode::Space => Key::Fire`, `ShiftLeft => Key::Boost`, `KeyQ => Key::RollLeft`, `KeyR => Key::Missile`, `KeyO => Key::Orbit` (`engine/platform/src/window.rs`).
- `Input` carries game concepts: `fire()`, `throttle()`, `roll_axis()`, `missile()`, `stick()`, and a second key type `Key` beside `Code`.
- IMAGINE then reads raw devices side by side in gameplay code (`input.down(Code::W)`, `input.button(Mouse::Left)`, `input.touch_region_held(touch::FIRE)`, `input.fire()`). It has to work around the engine: "the engine binds Space to the fire key, so the keyboard fire key is ignored while Space is held" (`games/imagine/game/src/present.rs`, `command`).
- Touch buttons are matched to gameplay by numeric constants in `games/imagine/game/src/touch.rs` that must agree with `data-id`s in the HUD.

**Why it was possible.** There is no input layer between devices and gameplay, so the engine guessed a game's bindings and each game reads devices directly. Every new device or rebinding is code in every game. The engine knowing a game's controls also breaks D24.

## How Unreal does it
Enhanced Input. Gameplay binds to **Input Actions** (assets with a value type: bool, axis 1D, 2D or 3D). **Input Mapping Contexts** map device keys to actions, with **modifiers** (dead zone, negate, swizzle, scale) and **triggers** (pressed, released, hold, tap). Contexts are added and removed with priorities as the player's state changes (on foot, in a menu, in a vehicle). Player rebinding writes a user mapping on top. Touch, gamepad, mouse and keyboard are all keys in the same map, and CommonUI builds its UI actions on the same system. ([Enhanced Input](https://dev.epicgames.com/documentation/unreal-engine/enhanced-input-in-unreal-engine))

## Design
1. **`engine-platform` input is raw device state only.**
   - Keyboard `Code`, mouse buttons and motion, touches, and gamepad buttons and sticks.
   - Delete `Key`, the window's game bindings, `fire`, `throttle`, `roll_axis`, `missile`, `stick` and `esc_quits`. Quitting is an action the game or the engine's menu handles.
2. **Actions in the engine.**
   - Put the new module where the layer rule allows; `engine-platform/src/input/actions/` is expected.
   - `ActionId` (newtype). `ActionValue::{Button(bool), Axis1(f32), Axis2(Vec2)}`, with the type fixed per action at load.
   - `MappingContext` as data: a list of (device input, action, modifiers, trigger).
     - Device input: a key `Code`, a mouse button, mouse delta, a gamepad button or stick, or a **touch control id**.
     - Modifiers: dead zone, negate, swizzle, scale.
     - Triggers: pressed, released, held, tap.
   - `ActionState` evaluates the active contexts once per frame, by priority. A device input claimed by a higher-priority context is consumed, as Unreal does.
   - It allocates nothing per frame: tables are built at load.
   - Gameplay reads `actions.value(id)`, `actions.started(id)`, `actions.triggered(id)` and `actions.completed(id)`.
3. **Data, checked at load.**
   - A game ships its actions and contexts in its content (`<game>/data/input.ron`, cooked into the pack by `asset-source`).
   - Load rejects, with errors that name the file and the entry:
     - an unknown action;
     - a value type mismatch (a key bound to an `Axis2` without a swizzle);
     - a duplicate binding in one context.
   - The engine ships a default context for its own controls (telemetry toggle, debug view modes) in the same format.
4. **Touch controls are actions.**
   - A HUD element declares `data-action="jump"`. The cooker resolves it to the action by name and rejects unknown names.
   - A drawn touch region carries its `ActionId`, and the touch-from-layout regions feed the same `ActionState` as keys.
   - Delete `games/imagine/game/src/touch.rs`'s numeric ids, and `drag_look` as a special flag: a drag is mapped to the `look` action's `Axis2` like mouse delta.
5. **Games use only actions.**
   - IMAGINE and Sandbox build their commands from actions alone; no `Code`, `Mouse` or touch id appears in game code.
   - Delete the Space workaround.
   - IMAGINE's contexts:
     - `on_foot`: move, look, jump, fire, aim, reload, crouch/slide, menu;
     - `menu`: ui-actions builds on this;
     - gamepad bindings for all of it.
6. **Rebinding** is a user context saved in the game's settings and layered on top. Provide the data path and its load check; the settings screen for it is `imagine-frontend-2`'s.

Tests:
- Evaluation per trigger and modifier.
- Priority and consumption.
- Load errors for each rejected case.
- A touch region drives the same action as its key.
- IMAGINE's command for (W + mouse left) equals the command for (left stick up + right trigger) and for (touch move + FIRE region).
- No allocation per frame.
- `rg 'Code::|Mouse::|TouchRegionId' games/*/game/src` finds nothing outside tests.

Starts after `asset-source` (data in the pack, start-up path) and `ui-css-layout` (HUD documents and `tools/ui-cook`) merge. `ui-actions` builds on this brief: its actions are these actions, so update `ui-actions.md` in the same change to say so.

Read first:
- `AGENTS.md`;
- `docs/README.md` §7 D24 and D69;
- `engine/platform/src/{input.rs, window.rs}`;
- `engine/ui` touch regions;
- `tools/ui-cook`;
- `games/imagine/game/src/{present.rs, touch.rs}`;
- `games/sandbox/game/src/present.rs`;
- `kits/fps/src/movement.rs`.

Docs in the same change:
- `docs/ARCHITECTURE.md` §1 (platform row);
- the decision written into `docs/ARCHITECTURE.md` §15 (input actions and mapping contexts, as Unreal Enhanced Input);
- `docs/briefs/README.md` status `review`.

## Gates
scripts/gate.sh
scripts/build-android.sh imagine
