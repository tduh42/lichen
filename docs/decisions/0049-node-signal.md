# ADR-0049 — Each node's signal, and the route to it

- Status: accepted
- Date: 2026-11-25

## Context

The radio screens are being redesigned (Nodes, a node's page, the map): the user wants to see at a glance which nodes the radio hears well, and how a node far away reaches it. lichen showed a node's type, hops and last heard (ADR-0041), and a trace's SNRs once asked for; it threw away the SNR and RSSI of every frame the radio reports in `LOG_RX_DATA`, except a group's envelopes for a search (§11.2). Stock companion firmware v1.18 (81dbae5) gives nothing more: no per-contact signal in its records, only the per-frame log.

## Decision

- **Only direct receptions are attributed** (protocol §16.1). A flood heard with 0 hops left its originator's radio: repeaters append their hash before passing a flood on. A relayed copy (1 hop or more) carries the signal of the last repeater, not of its sender: attributed to the sender, a node three repeaters away would show the hub's strong signal. A direct-routed packet arrives with an empty path whoever sent it last, since each repeater removes its own hash (oxymesh `air.md` §4.2): an empty path proves nothing there, so only adverts are taken from it (stock firmware sends them by flood or zero-hop, never along a path).
- **Adverts are attributed by their key**, once their signature verifies, and not when older than the last advert the radio reported for that node (a copy played again). **Other packets that name their sender** (REQ, RESPONSE, TXT_MSG, PATH) name it by one byte: they are attributed only when exactly one node lichen knows starts with it. A shared byte would put one node's signal on another.
- **The rating** compares the SNR with the demodulation limit of the current preset's spreading factor, −2.5 dB × (SF − 4): SF7 −7.5, SF8 −10, SF9 −12.5, SF10 −15, SF11 −17.5, SF12 −20 dB (Semtech, SX1261/2 datasheet, Table 6-1 "Range of Spreading Factors"; the same figures as the SX1276's Table 13).
  - SNR: **Good** at 10 dB above the limit or more, **Fair** at 5 dB above it or more, **Bad** below. A reading the radio barely decodes is Bad; 10 dB of margin rides out the usual fading of a few dB. At SF8: Good from 0 dB, Fair from −5 dB.
  - RSSI, when the radio measures it: Good from −115 dBm, Fair from −126 dBm, Bad below. It catches a quiet site where the SNR looks fine on a signal close to the receiver's floor; it does not depend on the preset, so its thresholds are loose and the SNR decides for most readings.
  - The rating is the worse of the two; the current preset rates every reading, older ones included. Integers in quarter dB, as the radio reports them; vectors in `testdata/lichen/signal.toml`, from an independent Python oracle.
- **History**: per radio and node, the readings of the last **7 days**, **500** at most (the newest). A week shows a repeater's day and night; 500 is a reading every 20 minutes for a week, and a few kilobytes per node. Kept per radio: a reading describes one radio's link to a node (ADR-0042 lets the phone change radios). It is the one thing of the mesh tools that is stored (ADR-0041 stored nothing): the radio keeps no signal per node, so it cannot be read again. Nothing else goes with it: no packet, no position of ours. Not in a backup (backup §3).
- **The route** (protocol §16.3) joins the node's path, the repeaters it names, the last trace and the first stop's own reading. A leg nothing measured says so; none is estimated.

## Consequences

- The Nodes list can show who is in reach and how well; a node's page can chart its readings and show its route leg by leg.
- A node heard only through repeaters has no signal of its own: its route says how it reaches us.
- Anyone who heard an advert can play it again near our radio (the same advert, at once): the signal shown is then theirs. It is shown, never used to route or to locate anyone.
- A 1-byte hash shared by two known nodes, or by a known node and an unknown one, loses or misplaces readings; adverts, the usual readings, are exact.
- Changing the preset rates the history again with the new limit.
- Rejected:
  - attributing relayed packets to their sender (the last repeater's signal);
  - attributing direct packets with an empty path (often the last repeater's);
  - rating against fixed SNR thresholds whatever the preset (−7 dB is comfortable at SF12 and undecodable at SF7);
  - keeping signal readings forever, or without the radio they came through.
