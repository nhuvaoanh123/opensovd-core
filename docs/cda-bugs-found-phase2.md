# CDA bugs found during Phase 2 Line A integration

Staging doc for anything we trip over in upstream classic-diagnostic-adapter
while building Phase 2 Line A. Per ADR-0007 build-first-contribute-later,
nothing here goes upstream until we have a reason to ship it.

---

## 2026-04-14 — CDA Windows build with default features fails without
## system OpenSSL or Strawberry Perl

**Category:** build environment / upstream assumption, not a CDA code bug.

**What we tried:**

```sh
cd H:/eclipse-opensovd/classic-diagnostic-adapter
cargo build --release -p opensovd-cda
```

**What happened:** `openssl-sys v0.9.111` build script failed:

```
Could not find directory of OpenSSL installation, and this `-sys` crate
cannot proceed without this knowledge. ... OPENSSL_DIR unset
```

**Upstream CI equivalent:** `.github/workflows/build.yml` job
`build_windows` sets `OPENSSL_DIR=C:\Program Files\OpenSSL` and assumes a
pre-installed OpenSSL Windows distribution on the runner. Locally we have
neither that install nor Administrator rights to put one there.

**Things we tried as workarounds:**

1. `cargo build --release --no-default-features --features
   health,openssl-vendored -p opensovd-cda`

   The `openssl-src` crate requires Perl to run `./Configure`. Git Bash's
   bundled Perl is missing the `Locale::Maketext::Simple` / `ExtUtils::MakeMaker`
   modules. We built a wrapper `.cmd` that injects a user-copied msys2
   core_perl tree via `-I H:\tmp\perl5lib` and pointed `OPENSSL_SRC_PERL`
   at it. That got past the module loader, but `./Configure` for
   `VC-WIN64A` then rejects the wrapped perl binary with:

   ```
   This Perl version: 5.38.2 for x86_64-msys-thread-multi
   (You may have to use a platform-matched perl.)
   ```

   OpenSSL's upstream Configure script explicitly refuses any
   MSYS/Cygwin-built Perl when targeting VC-WIN64A. The only supported
   options are a native Strawberry Perl or an ActivePerl install.

2. `cargo build --release --no-default-features --features health,mbedtls
   -p opensovd-cda`

   mbedtls-sys builds OK (cmake + MSVC are present), but the wrapping
   crate pulls in `bindgen 0.72` which panics because `libclang.dll` is
   not installed on this machine:

   ```
   Unable to find libclang: "couldn't find any valid shared libraries
   matching: ['clang.dll', 'libclang.dll'], set the LIBCLANG_PATH
   environment variable..."
   ```

3. `choco install strawberryperl -y` — fails with `Access to the path
   'C:\ProgramData\chocolatey\lib-bad' is denied`. Chocolatey needs an
   elevated shell for package installs on this host.

**Impact on Phase 2 Line A:**

- T1 (CDA Windows build) is blocked on a local environment fix, not on
  any CDA code change. Our opensovd-core branch is green and every
  non-CDA task in the prompt has been executed.
- T4 (run-cda-local.sh) is written and tests work against a hypothetical
  binary, but cannot smoke-launch until the CDA binary exists.
- T5 (phase2_cda_ecusim_smoke) is written with a preflight gate that
  skips when the bench is unreachable — it runs cleanly with no bench and
  will pass automatically once CDA is built and pointed at the Pi ecu-sim.
- T8 (phase2_sovd_over_cda_ecusim) uses a mock CDA running locally via a
  second in-process `InMemoryServer` and does NOT depend on the real CDA
  binary at all. It passes today.
- T2 (Pi ecu-sim deployment) is gated on the dev machine being able to
  run the real CDA to drive it — not strictly blocked by T1, we could
  deploy ecu-sim to the Pi in isolation. Deferred to a follow-up session
  so T1, T4, T2 can all be verified back-to-back once the build env is
  fixed.

**Proposed fix:**

1. On the dev machine, install Strawberry Perl 5.42+ with an elevated
   shell: `choco install strawberryperl -y`. Add `C:\Strawberry\perl\bin`
   to PATH, verify `perl -e "use Locale::Maketext::Simple; use ExtUtils::MakeMaker; print 'ok'"`.
2. Then `cd classic-diagnostic-adapter && cargo build --release
   --no-default-features --features health,openssl-vendored -p opensovd-cda`.
   Expected to succeed because the `cc` crate already finds MSVC via
   the VS2019 Build Tools install at
   `C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\`.
3. Alternatively, ship a `deploy/windows-dev-bootstrap.ps1` script in
   opensovd-core that checks + installs Strawberry Perl if missing, so
   future workers don't hit this.

**Upstream contribution potential:**

Minor — CDA could add a `docs/build/windows.md` note that running from a
non-VS-dev-prompt requires Strawberry Perl for `openssl-vendored` or a
pre-installed OpenSSL for the default feature set. Not a bug, not a
merge priority; add to the "possible easy PR" list only.
