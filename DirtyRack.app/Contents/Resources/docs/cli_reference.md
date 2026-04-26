# DirtyRack Forensic CLI Reference (Draft)

`dirtyrack-cli` is a command-line tool for deterministic audio rendering and forensic validation of patch identity.

## Basic Commands

### `dirtyrack render <PATCH_JSON> [OPTIONS]`
Renders the specified patch file offline and outputs a deterministic audio file.
- `--output <FILE>`: Output filename (.wav).
- `--length <SEC>`: Length of the audio to render.
- `--sample-rate <HZ>`: Sample rate.
- **Feature**: The hash value of the rendering result is calculated and verified for identity against previous takes.

### `dirtyrack verify <PATCH_JSON> <HASH>`
Verifies that the rendering result of the patch matches the specified hash value bit-for-bit. Used for regression testing in CI/CD environments.

### `dirtyrack gui`
Launches the main graphical projector (GUI).

## Developer Commands

### `dirtyrack module list`
Displays a list of currently loadable built-in and third-party modules (inside the `modules/` folder).

### `dirtyrack sdk init <DIR>`
Creates a template project for a new third-party module development in the specified directory.

---

> [!NOTE]
> Currently, DirtyRack focuses on intuitive patching via the GUI, but the deterministic engine equivalent to this CLI always operates behind the scenes to guarantee the reproducibility of all operations.
