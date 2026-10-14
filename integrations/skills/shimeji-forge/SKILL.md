---
name: shimeji-forge
description: Generate a shimeji pack with the agent's own image tool, then extract, place, check, derive, review, and install it with the forge CLI. Never print environment variables.
---

# shimeji-forge

Never print environment variables. Do not read them, echo them, or write them into files, prompts, or tool arguments. This skill does not use a provider key.

Forge's paid image providers are not part of this flow. Generate each sheet with the image tool built into Codex or Claude Code.

## Steps

1. Write the job list:

   ```
   forge agent-jobs --out <jobs-dir> --character NAME
   ```

   `jobs.json` lists groups G1–G8. Each job's `output` is `{id}.png` in that directory (`G1.png` … `G8.png`). Guides are `guides/{id}.png`. Prompt files are the job's `prompt`, `base_prompt`, and `strip_prompt`.

2. For each job, generate one image with your own image tool. The sheet is the poses named in the job's `notes`, in slot order, on the green background `#00FF00`. Write the PNG to the job's `output` path. Do not invent a second output name.

3. Extract, place, and check each group. `jobs.json` is the list of groups:

   ```
   forge extract <jobs-dir>/G1.png --group G1 --out <frames>/G1
   forge place --sheet <jobs-dir>/G1.png --group G1 --out <pack>/img
   forge qa --sheet <jobs-dir>/G1.png --group G1 --out <qa>/G1
   ```

   Repeat for G2–G8. `forge qa` exits 1 when a check fails. Fix the sheet and run that group again. Do not pass a provider key.

4. Composite the split frames and clip the carry frames:

   ```
   forge derive <pack>
   ```

   This rewrites `img/shime42.png`–`img/shime46.png` from frame 1, and clips `img/shime34.png`–`img/shime36.png`.

5. Look at the result:

   ```
   forge review <pack> --out <review-dir>
   ```

   `contact-sheet.png` labels cells 1–46. One GIF per animation uses the durations from the pack XML. Add `--open` to open the sheet in Preview.

6. Install only through the app's confirmation:

   ```
   forge install <pack>
   ```

   The app asks `forge wants to install NAME from PATH: Install / Ignore`. Nothing is copied until Install is clicked. If no app is listening, forge prints the folder to drag in and exits 0.

Never print environment variables.
