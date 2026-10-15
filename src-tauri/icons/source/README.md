# Icon source images

The platform icons in the parent folder (`icon.ico`, `icon.icns`, the `*.png` sizes, and the
`android/` and `ios/` folders) are generated from a source image with the Tauri CLI. These are
the source images kept for regenerating them. They are not bundled into the app.

| File | Size | Notes |
|------|------|-------|
| `app-icon-source.png` | 1024×1024 | Regenerating from this file reproduces the committed `icon.ico` byte for byte. Use this one. |
| `app-icon-source-macos.png` | 1024×1024 | macOS-styled variant (committed 2026-04-05). Regenerating from it does not reproduce any committed icon; it is unused as far as we know. |
| `app-icon-new.png` | 256×256 | Small draft (committed 2026-04-05). Too small to be a source; unused as far as we know. |

To regenerate the icons (this overwrites the files in the parent folder):

```bash
npm run tauri icon src-tauri/icons/source/app-icon-source.png
```

The two unused images can be deleted once someone confirms they are not wanted.
