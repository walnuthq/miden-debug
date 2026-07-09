---
title: Recording and replaying transactions
sidebar_position: 7
---

# Recording and replaying transactions

A DAP session driven by `miden-client` can **record** a transaction — the program,
its inputs, the resolved code, and every advice mutation produced by the transaction
host's event handlers — into a self-contained *replay snapshot*. The snapshot can then
be **replayed** offline in the `miden-debug` TUI, with no node, client, or account
state, so you can step through the exact same execution as many times as you like.

This is the simplest way to debug a real note-consumption transaction (e.g. a P2ID
note) end to end.

Both tools below are the locally built binaries: `miden-client` from the
[miden-client](https://github.com/0xMiden/miden-client) repo (built with the `dap`
feature) and `miden-debug` from this repo.

## Record a note-consumption transaction

The example uses the public testnet, so no local node is needed.

### 1. Create and fund a wallet

```bash
STORE="$HOME/miden-p2id-testnet"
rm -rf "$STORE" && mkdir -p "$STORE"

HOME="$STORE" miden-client init --network testnet
HOME="$STORE" miden-client new-wallet
WALLET=$(HOME="$STORE" miden-client account -l | grep -oE '0x[0-9a-f]+' | head -1)
echo "Fund THIS exact ID at the faucet: $WALLET"
```

Go to the [Miden testnet faucet](https://faucet.testnet.miden.io/), paste `$WALLET`, and
click **Send Public Note**. The faucet sends a P2ID note to your wallet.

> Keep using the same absolute `HOME="$STORE"` on every command — it pins the client's
> store to one location. And fund the *exact* ID that `account -l` prints; a P2ID note
> asserts that the consuming account equals the note's target.

### 2. Find the note

```bash
HOME="$STORE" miden-client sync                              # wait ~20s after funding
HOME="$STORE" miden-client notes -l consumable -a "$WALLET"  # copy the Note ID it lists
```

### 3. Debug and record the consumption

The `consume-notes` command runs the transaction under a DAP server instead of proving
and submitting it, and `--record` writes the replay snapshot when the session ends.

**Terminal 1** — start the debug adapter (replace `<NOTE_ID>` with the ID from step 2):

```bash
HOME="$STORE" miden-client consume-notes -a "$WALLET" <NOTE_ID> \
  --start-debug-adapter 127.0.0.1:4711 \
  --record "$STORE/p2id.mdsnap"
```

**Terminal 2** — attach the debugger and step through the transaction (kernel → note
script → the wallet's `receive_asset`); press `c` to run to the end, `q` to quit:

```bash
miden-debug --dap-connect 127.0.0.1:4711
```

When the session ends, Terminal 1 reports the snapshot:

```text
Wrote replay snapshot (542 event(s), 5 forest(s)) to .../p2id.mdsnap
Recorded 542 advice mutation set(s) from event handlers during the debug session.
Wrote replay snapshot to .../p2id.mdsnap; replay it with `miden-debug --replay .../p2id.mdsnap`.
```

## Recording without a debugger

`--record` also works **without** `--start-debug-adapter`: the transaction executes
headlessly while recording — nothing to attach, no stepping — and the snapshot is written
in a single command. Given without a file, the snapshot is stored keyed by the executed
transaction's ID (under `$MIDEN_DEBUG_SNAPSHOTS` or `~/.miden/debug-snapshots`):

```bash
HOME="$STORE" miden-client consume-notes -a "$WALLET" <NOTE_ID> --record
```

```text
Recorded transaction 0x6f63a2e5e18b705c38b1b75845c65a287e607f5d2a9217f3c0e7da1104fb36f0.
Trace it with `miden-debug --trace 0x6f63a2e5...` or replay it with `miden-debug --replay 0x6f63a2e5...`.
```

Pass `--record <FILE>` (or `--record=<FILE>`) to write to an explicit path instead. Like
the debug session, neither form proves, submits, or applies the transaction.

## Replay offline

```bash
miden-debug --replay <TX_ID>            # a recorded transaction (unique prefixes work)
miden-debug --replay path/to/tx.mdsnap  # or an explicit snapshot file
```

Transaction IDs are resolved against `$MIDEN_DEBUG_SNAPSHOTS`, `./.miden/debug-snapshots`,
and `~/.miden/debug-snapshots`. Only locally recorded transactions can be resolved this
way — the chain does not carry the execution inputs a snapshot captures, so an arbitrary
on-chain transaction ID is not traceable.

The recorded events are fed back through the debugger's event-replay host, so you step
through the identical execution — no network or wallet required. The snapshot carries no
source files, so the debugger shows disassembly.

## Print a function trace

`--trace` re-executes a snapshot headlessly and prints every function the transaction
executes, in order, followed by a per-function cycle summary:

```bash
miden-debug --trace <TX_ID>             # or an explicit snapshot file path
```

```text
Function trace: 6000 transition(s), 272 unique function(s), 79131 cycle(s)

     cycle  function
         0  ::$exec::$main
        11  ::$kernel::prologue::prepare_transaction
       ...
      3948  ::$kernel::note::prepare_note
      3976  ::miden::standards::notes::p2id::main
       ...

Functions by self-cycles:
    cycles  entries  function
     19582      512  ::miden::core::crypto::dsa::falcon512_poseidon2::mod_12289
       ...
```

Function names come from the debug info embedded in the executed code; segments without
debug info are attributed to `<unknown>`. If the recorded run failed, the trace up to the
failure point is printed before the error.

## Replaying a failed transaction

A snapshot is written even when the debugged transaction **fails** mid-execution (for
example, a note-script assertion). This captures the run up to the failure point, so you
can replay a failing consume offline — or `--trace` it — and step right up to where it
went wrong — often the most useful case to debug.
