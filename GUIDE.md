# Beginner's guide: from zero to a real app on GitHub

You never need to understand everything at once. Follow the steps in order.

## The big picture

| Word | What it means |
|---|---|
| **Source code** | Text files (`.rs` = Rust) that describe what the program does. You (or I) write these. |
| **Compiler** (`rustc`, run by `cargo`) | A program that turns source code into an `.exe` Windows can run. |
| **Cargo** | Rust's all-in-one tool: downloads libraries, compiles, runs. |
| **Git** | Saves snapshots ("commits") of your project, so you can go back in time. |
| **GitHub** | A website that stores your Git project online, so it is backed up and shareable. |
| **GitHub Actions** | Free robots on GitHub that build your app on a clean Windows machine every time you upload. |

The project is a folder. Everything lives inside it:

```
pico-widgets/
  Cargo.toml                  <- the project's ID card: name, version, libraries it uses
  src/
    main.rs                   <- where the program starts
    app.rs                    <- Windows window, dragging, right-click menu, timer
    draw.rs                   <- draws the clock into pixels (no Windows knowledge)
  .github/workflows/build.yml <- tells GitHub how to build your app
  .gitignore                  <- files Git should NOT save (like the big "target" build folder)
  README.md                   <- the front page of your GitHub project
  GUIDE.md                    <- this file
  LICENSE                     <- what others may do with your code
```

---

## Step 1: Install the tools (once, ~20 minutes)

1. **Rust**: go to <https://rustup.rs>, download and run `rustup-init.exe`, press Enter to accept the defaults.
   It will say it needs the **Visual Studio C++ Build Tools** and offer to install them. Say yes.
   In the installer, make sure **"Desktop development with C++"** is ticked. It is a big download (a few GB). That's normal.
2. **Git**: <https://git-scm.com/download/win>. Click Next through everything.
3. **VS Code** (the editor where you read and edit code): <https://code.visualstudio.com>.
   Open it, click the Extensions icon (four squares on the left), and install **rust-analyzer**.
4. **Restart your PC** (or at least sign out and in) so everything is found.

Check it worked: open **PowerShell** (Start menu, type "PowerShell") and run:

```
cargo --version
git --version
```

Both should print a version number.

## Step 2: Put the code on your computer

1. Make a folder for your projects, for example `C:\Users\<you>\Projects`.
2. Unzip `pico-widgets.zip` there, so you end up with `C:\Users\<you>\Projects\pico-widgets\Cargo.toml`.
   (`Cargo.toml` must sit directly inside the folder you open. Not one folder deeper.)
3. Open VS Code, then **File -> Open Folder** and choose `pico-widgets`. Click "Yes, I trust the authors".

## Step 3: Run it

In VS Code open the terminal (**Ctrl + `**, the key under Esc) and run:

```
cargo run --release
```

The first time it downloads libraries and compiles, which takes a few minutes. Then a clock widget appears at the top-right of your screen.

- **Drag** it with the left mouse button.
- **Right-click, then Quit** to close it.

The finished program is at `target\release\pico-widgets.exe`. Double-click it any time. It is the real app.

If you see red error text instead: copy **all** of it and send it to me. Errors are normal, they are how the compiler tells us what to fix. The first build of brand-new code very often needs one small fix.

## Step 3b: Change something (this is how you learn)

Close the widgets first (right-click one, then **Quit**), then open `pico-widgets.json` next to the exe, or right-click a widget and choose **Settings...** to use the settings window (the recommended way).

Change `"background": "#1C1C20"` to `"background": "#1F3A6E"` and `"radius": 28` to `"radius": 60`, then save. The widget updates within a second, with no restart needed.

- `radius`: how round the corners are (0 = square)
- `opacity`: 0.0 invisible to 1.0 solid
- `background` and `text_color`: colors as `#RRGGBB`
- `show_date`: `true` or `false`
- For a photo widget, add `{ "type": "photo", "width": 240, "height": 240, "radius": 40, "path": "C:\\Users\\you\\Pictures\\cat.jpg" }`. Use double backslashes in paths.
- For a notepad widget, choose **Add note** in Settings, then type in its **Note** tab and click **Save**. Text is stored in the widget's `text` field.
- For Spotify, create an app in the [Spotify Developer Dashboard](https://developer.spotify.com/dashboard), add `http://127.0.0.1:43821/callback` as a Redirect URI, then paste its Client ID into the **Spotify** tab in Settings and connect. After this update, reconnect once to grant playback-device access. Add a music widget from the sidebar. Select it and use the **Photo** tab to choose a background image. Play/pause/skip require Spotify Premium; open Spotify on a device at least once so it appears in the device list.

If the file has a mistake, a message box tells you which line to look at.

## Step 4: Put it on GitHub

1. Create a free account at <https://github.com>.
2. Click **+ -> New repository**. Name it `pico-widgets`. Leave it **Public** (the free build robots work best that way). **Do not** tick "Add a README" or any other box. Click **Create repository**.
3. Tell Git who you are (once). In the VS Code terminal:

   ```
   git config --global user.name "Your Name"
   git config --global user.email "you@example.com"
   ```

4. In the terminal, inside the `pico-widgets` folder, run these one at a time:

   ```
   git init
   git add .
   git commit -m "First version: clock widget"
   git branch -M main
   git remote add origin https://github.com/<your-username>/pico-widgets.git
   git push -u origin main
   ```

   (Use the exact URL GitHub shows you on the empty repository page.)
   The first push opens a browser window asking you to sign in to GitHub. Approve it.

What each command means: `add .` = "include all my files", `commit` = "take a snapshot with this label", `push` = "upload the snapshots".

> **Important:** the first `cargo run` created a file called `Cargo.lock`. Keep it and commit it. It pins the exact library versions so your builds are repeatable. It was included by `git add .` above.

## Step 5: Watch GitHub build it for you

On your repository page click the **Actions** tab. You'll see a "Build" run with a yellow dot (running), then a green tick (success) after a few minutes. Click the run, scroll down to **Artifacts**, and download `pico-widgets-windows`. That zip contains the `.exe`. A red cross means the build failed: click it, copy the error, and send it to me.

## Step 6: Publish a real release (the download button for users)

```
git tag v0.1.0
git push origin v0.1.0
```

GitHub builds the app again and attaches `pico-widgets-windows.zip` to a **Release**. Look at the **Releases** section on the right side of your repository page. Share that link with anyone.

> Windows may show a blue "SmartScreen" warning for new apps from unknown publishers, because the exe is not code-signed (signing costs money). Users click **More info -> Run anyway**.

## Your everyday loop

1. Edit code in VS Code, save.
2. `cargo run --release` to try it.
3. When happy: `git add .`, then `git commit -m "what I changed"`, then `git push`.
4. Check the green tick on the Actions tab.
5. For a new public version: bump `version` in `Cargo.toml`, commit and push, then `git tag vX.Y.Z` and `git push origin vX.Y.Z`.

## How this keeps RAM low (what to remember when we add features)

- No big UI framework, no web browser inside the app, no GPU. We draw pixels directly.
- The program sleeps until Windows wakes it. The clock wakes once a minute.
- Photo widgets will decode pictures at the widget's size, never full resolution.
- A single process will host all widgets, instead of one process per widget.
- We will measure memory in Task Manager (Details tab, column "Memory (private working set)") after every milestone.

## Troubleshooting

| Problem | What to do |
|---|---|
| `cargo` is not recognized | Close and reopen the terminal, or restart the PC. |
| `link.exe not found` | The C++ Build Tools are missing. Re-run the Visual Studio Installer and tick "Desktop development with C++". |
| The widget text is missing | The app reads Segoe UI from `C:\Windows\Fonts`. Tell me and we'll bundle a font. |
| `git push` asks for a password | Use the browser sign-in prompt. GitHub no longer accepts account passwords in the terminal. |
| Anything else | Copy the whole message and send it to me. |

## About the license

`LICENSE` is the MIT license, with a placeholder name ("pico-widgets contributors"). MIT means anyone can use and modify your code as long as they keep your notice. Replace the name with yours if you like. I'd change the name of the project too if you want something more unique.
