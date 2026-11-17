# ui-document

Status: merged (written directly by the architect, not delegated). Decision: D46.

## What

A UI screen is data: a `*.ui.ron` document, laid out with `taffy`, filled from game variables, drawn through a flat draw list. An HTML and CSS subset compiles to the same document.

## Design

- `engine/ui`: `doc` (types, validation, `from_ron`, `to_ron`), `bindings` (`Value`, `Bindings`), `template` (`{name}`, `{name:N}`), `layout` (`UiLayout::frame` to `DrawList` and `ButtonRegion`s, `TextMeasure`).
- `engine/app`: `UiScreen` (draw onto `render::Ui`, `reload_if_changed`, last good document kept on error).
- `tools/ui-cook`: `cook(html) -> RON`, `ui-cook page.html out.ui.ron`; sample `examples/hud.html`.

## Done when

Gates pass; layouts tested at 16:9 and 20:9; hot reload tested without sleeps; every unsupported HTML or CSS construct has a named error test.

## Not done

Port the IMAGINE and sandbox HUDs and touch buttons to documents; reinstall touch regions on resize; measure layout cost on a phone.
