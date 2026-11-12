# Menu draw-list goldens

`home-menu-desktop.golden` and `home-menu-phone.golden` capture the retained draw
lists and text for the fixture in `tests/menu_golden.rs` at 1920×1080 and 844×390.

After an intentional rendering change, regenerate both with:

```sh
UPDATE_MENU_GOLDENS=1 cargo test -p engine-ui --test menu_golden
```

Run the same test without the environment variable to compare against the checked-in
files. The fixture also asserts that a warmed steady frame allocates no memory at
both sizes.
