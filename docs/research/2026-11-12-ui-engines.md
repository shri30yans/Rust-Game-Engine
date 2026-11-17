# UI engines compared: what we take from Unreal, Unity, Bevy, Godot and game UI middleware

> Dated evidence, not the source of truth. The current design is in the topic docs listed in [../README.md](../README.md) §1; old doc names and decision numbers cited below refer to docs merged on 2026-11-17 (see [../README.md](../README.md#7-former-decision-numbers) §7).

Written 2026-11-12, when the owner asked: "compare the UI engines of Bevy and other platforms; see what we can copy and how to make our system much higher quality." It covers our UI as it stood that day: `engine/ui`, `tools/ui-cook`, the UI pass in `engine/render`, and IMAGINE's documents.

The rule from D55 applies: when in doubt, follow Unreal. Where another system has a simpler, correct-by-construction answer, take that instead.

## 1. Where we stand

| Area | Ours today |
|---|---|
| Authoring | HTML and CSS subset, cooked by `tools/ui-cook` into a validated document (D46). Hot reload on desktop. |
| Layout | Flexbox (taffy), retained; it re-runs only when something changes (audit H11). Intrinsic text size, then shrink and ellipsis (D65). |
| Look | Theme tokens (RON), state styles, skew for the angled HUD (D48). Being added by `ui-kit-2`: images, nine-slice, gradients, clipping, scroll, font faces, safe areas. |
| Data | `Bindings`: a map from name to a `Value` (number, flag, text, list) that the game sets each frame. Names and types are checked when the document reads them, at run time. |
| Screens and input | `ScreenStack` with layers (Game, GameMenu, Menu, Modal, Toast); only the top screen that takes input handles a frame; spatial focus navigation; timed screen transitions. |
| Touch | HUD touch buttons came from a second, fraction-based layout. That path caused the phone crash; `touch-from-layout` replaces it with hit tests against the drawn layout. |
| Tools | `ui-preview` (brief): live window, shots, comparison against references, checks. Not built yet. |

## 2. What each system does well

**Unreal (UMG, Slate, CommonUI, Viewmodel).**
- **CommonUI activatable widgets.** Screens form an activation tree. Input goes only to the topmost active widget, and a modal widget becomes a new input root. Each widget declares its input config: whether game input passes through, mouse capture, and cursor visibility. The previous config is restored when it deactivates.
- **CommonUI actions and input glyphs.** A screen declares actions (Back, Confirm, Tab left). An action bar shows the right button prompt for the device in use (touch, gamepad, keyboard) and switches the moment the player changes device.
- **Viewmodel (MVVM).** A typed object with `FieldNotify` properties. Widgets bind to fields, a change notification marks only the dependent widgets, and there is no per-frame polling (polling is a known pitfall).
- **Invalidation.** Slate caches paint, layout and hierarchy separately and invalidates each only on a matching change.
  - *Invalidation Box* covers a subtree, and *Global Invalidation* covers the whole window.
  - *Retainer Panel* renders a subtree to a texture, at a lower rate if wanted.
  - Animating paint-only properties (colour, opacity, render transform) is cheap; animating layout is expensive.
- **Styles as data assets.** Text, button and border styles are shared assets that widgets reference, never per-widget values.

Sources: [CommonUI input guide](https://dev.epicgames.com/documentation/unreal-engine/commonui-input-technical-guide-for-unreal-engine), [CommonUI overview](https://dev.epicgames.com/documentation/en-us/unreal-engine/overview-of-advanced-multiplatform-user-interfaces-with-common-ui-for-unreal-engine), [UMG Viewmodel](https://dev.epicgames.com/documentation/unreal-engine/umg-viewmodel-for-unreal-engine?lang=en-US), [UMG optimisation (invalidation, retainer)](https://dev.epicgames.com/documentation/en-us/unreal-engine/optimization-guidelines-for-umg-in-unreal-engine), [MVVM polling pitfall](https://forums.unrealengine.com/t/umg-polling-mvvm-binding-every-frame-instead-of-only-when-the-delegate-fires/1190201).

**Unity (UI Toolkit).**
- Markup (UXML) and style sheets (USS) are kept apart. Style sheets have variables, selectors and transitions.
- **Runtime data binding.** A binding can target a property, a USS style or an attribute, and is declared in the document. Property bags are generated from reflection, which costs a little at first use.
- **Live reload.** It is on by default in the Game view and recreates the UI. A reload callback restores state, so state must live outside the view.
- **UI Builder.** A visual editor that writes UXML.

Sources: [Live reload](https://docs.unity3d.com/Manual/ui-systems/ui-toolkit-live-reload.html), [Runtime binding](https://docs.unity3d.com/Manual/UIE-runtime-binding-examples.html), [Data binding best practices](https://docs.unity3d.com/Manual/best-practice-guides/ui-toolkit-for-advanced-unity-developers/data-binding.html), [UI Toolkit status, Nov 2025](https://discussions.unity.com/t/ui-toolkit-development-status-and-next-milestones-november-2025/1698009).

**Bevy (bevy_ui, bevy_ui_widgets, Feathers, BSN).**
- **Headless widgets.** `bevy_ui_widgets` gives buttons, checkboxes, sliders, scroll views and tabs with behaviour and accessibility state but no look. Feathers is a styled set on top, and Feathers is explicitly *not for game UI*. Behaviour and look are separate layers.
- **BSN.** A scene notation that makes "headless core plus a styled patch" cheap to compose.
- **The rest.** Native `BoxShadow`, gradients, picking-based UI input, automatic directional navigation, and tab order.
- **To avoid.** Building UI by spawning entities in code is verbose, and designers cannot edit it. Our documents are better here.

Sources: [Feathers gallery](https://bevy.org/examples/ui-user-interface/feathers-gallery/), [bevy_ui_widgets](https://docs.rs/bevy_ui_widgets), [Bevy 0.20 notes](https://bevy.org/news/bevy-0-20/), [Directional navigation](https://bevyengine.org/examples/ui-user-interface/directional-navigation), [bevy_ui docs](https://docs.rs/bevy_ui).

**Godot (Control, Theme).**
- **Theme type variations.** A named variant ("PrimaryButton", "DangerButton") extends a base type and overrides only some properties. Variations chain, and a node picks one by name.
- The theme cascades through a subtree.
- Container nodes (HBox, Grid, Margin) carry the layout rules, so positions are never set by hand.

Sources: [Theme type variations](https://docs.godotengine.org/en/4.2/tutorials/ui/gui_theme_type_variations.html), [Theme class](https://docs.godotengine.org/en/4.2/classes/class_theme.html).

**Game UI middleware: Gameface (HTML), RmlUi (HTML), Noesis (XAML).**
- **Declarative data binding in markup**, structural as well as values:
  - RmlUi: `{{value}}`, `data-if`, `data-for`, `data-value`;
  - Gameface: `data-bind-*`.
  The data model is registered once.
- **Developer tools** (Gameface): an inspector shows every binding with its live value, allows editing CSS live, and profiles layout and paint per frame.
- **Virtual lists** (Gameface): long lists create only the visible rows.
- Noesis uses MVVM with XAML, the same idea as Unreal's Viewmodel.

Sources: [RmlUi](https://github.com/mikke89/RmlUi), [RmlUi data binding examples](https://mikke89.github.io/RmlUiDoc/pages/data_bindings/examples.html), [Gameface features](https://coherent-labs.com/all-features-gameface), [Gameface dev tools](https://guide.coherent-labs.com/planning-and-setup/development-environment-and-tools), [Noesis](https://www.noesisengine.com/noesisgui/).

## 3. Where we fall short, and what to take

Each item is a defect class that the design should rule out, not a feature to copy.

| # | Gap in ours | Who does it right | What we do |
|---|---|---|---|
| U1 | **Bindings are typed only at run time.** A mistyped or missing name is an error the player can hit. Every value is pushed every frame. | Unreal Viewmodel, Unity runtime binding, RmlUi data model | **Typed view models.** The game declares `#[derive(ViewModel)] struct HudModel { … }`. The cooker emits each document's binding schema (names, types, list row shapes). A document is bound to a model **once, when it loads**, and a mismatch is a load error caught by tests and `ui-preview check`, never a frame error. Fields carry change flags: a frame with no changes does no binding work, and a text change marks only the nodes that read it. Brief `ui-viewmodel`. |
| U2 | **Input context is scattered.** Each screen decides by hand whether game input passes through, whether the cursor shows, and how Back works. | CommonUI activatable widgets and input config | **Screens declare an input mode:** `Game`, `Menu` or `GameAndMenu`, plus cursor and capture. The stack applies the top screen's mode and restores the previous one when it pops, so the game never sets input state itself. Brief `ui-actions`. |
| U3 | **No button prompts that follow the device.** | CommonUI actions and action bar | **Screens declare actions** (`back`, `confirm`, `tab_prev`, `tab_next`, `ready`). An engine action bar widget shows the glyph for the current device (touch, gamepad family, keyboard) and switches the moment the device changes. Brief `ui-actions`. |
| U4 | **Styles live in each document.** Nothing stops a raw colour or size outside the design system. | Godot type variations, Unreal style assets, USS variables | **Style variants as data.** The theme defines components (`button.primary`, `button.ghost`, `panel.card`) as token sets. Documents use `class="button primary"`. The cooker rejects raw colours, sizes and fonts outside tokens (a lint, with the theme file as the only exception), so the design system holds by construction. Fold into `imagine-frontend-2`'s design system and the ui-cook lint. |
| U5 | **Every animation can trigger layout.** | Slate invalidation; the web's compositor-only properties | **Paint-only motion.** Transitions and keyframes may animate only `opacity`, `transform` (translate, scale, rotate) and colours, which change paint, never layout. A layout property in a transition is a cook error. CSS `@keyframes` subset plus `transition`. Brief `ui-motion`. |
| U6 | **Lists build every row.** | Gameface virtual lists, Unreal ListView | **Virtual list:** a repeated template inside a scroll view creates only the visible rows, plus one row of buffer, and reuses them. Add to `ui-kit-2`'s follow-up, or `ui-motion`, whichever is next. |
| U7 | **No way to see bindings and boxes in the running game.** | Gameface inspector, Unity UI debugger | **An inspector overlay in `ui-preview` and in desktop debug builds:** hover shows the element, its box, its classes and its bound fields with live values; a frame timer shows layout and paint cost. Add to `ui-preview`. |
| U8 | **Behaviour and look are mixed in widget code.** | Bevy headless widgets plus Feathers | Keep widget behaviour (button, toggle, slider, tabs, scroll, list) as headless engine state machines with visual states. Everything visual comes from the document and theme. Our design already points this way; state it as a decision, and apply it to the widget work in `ui-kit-2`. |
| U9 | **Text shaping and localisation are missing** (audit M6). | Bevy (cosmic-text), Unity (TextCore), Unreal (HarfBuzz) | Later: `parley` for shaping, bidi and fallback fonts, and MSDF glyphs. Needed before any non-Latin language. Not now. |
| U10 | **No accessibility tree.** | Bevy (AccessKit), Unity, Unreal | Later: export the focus tree to AccessKit on desktop and mobile. Today's text scale, contrast and reduced-motion settings stay. |

**What we already do as well or better:**
- one validated document format, cooked once;
- retained layout, re-run only on change;
- the screen stack with layers and focus restore (CommonUI-like);
- hot reload;
- text fit rules;
- a test that checks text overflow at four screen sizes;
- after `touch-from-layout`, input tested against drawn geometry.

**What we do not take, and why:**
- **Embedded Chromium or WebView:** too heavy on phones, with a second renderer.
- **Immediate-mode UI (egui) for the game:** keep it for dev tools only.
- **ECS-spawned UI in code** (Bevy style): designers cannot edit it.
- **A visual editor like UMG Designer or UI Builder:** HTML in an editor with `ui-preview` live is faster for us, and agents can drive it.

## 4. Order

1. `touch-from-layout` (running) and `android-lifecycle`: the crashes first.
2. `ui-kit-2` (running): images, icons, nine-slice and gradients. Then `ui-kit-3`: clipping, scroll, font faces, safe areas.
3. `ui-viewmodel` (U1): typed models and load-time binding checks. Before any new screen is built, so `imagine-frontend-2` binds through it.
4. `ui-preview`, with the inspector (U7), and at the same time `ui-actions` (U2, U3). The preview is mostly a new tool, so it can run beside an `engine/ui` brief.
5. `ui-motion` (U5, U6): paint-only transitions and keyframes, and virtual lists.
6. `imagine-frontend-2`, using all of the above, with style variants and the token lint (U4).

`ui-viewmodel`, `ui-actions` and `ui-motion` all edit `engine/ui`, so they run one at a time after `ui-kit-2`.
