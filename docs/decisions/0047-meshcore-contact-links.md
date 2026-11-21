# ADR-0047 — A MeshCore contact from a link

- Status: accepted
- Date: 2026-11-21

## Context

A user asked to add a MeshCore contact from the link the stock app shares (F-2): `meshcore://` followed by the hex digits of a whole packet, the node's signed advert. MeshCore's documentation at 81dbae5 describes the `contact/add` code (§15.3, F-1) but not this form; its bytes are what the companion's `EXPORT_CONTACT` returns, and `IMPORT_CONTACT` takes them back (§16.6). The firmware settles what each command does, but leaves three things to the app:

- `IMPORT_CONTACT` answers `OK` as soon as the bytes read as an advert. Whether the radio then adds the node depends on its signature, its settings (manual add, the auto-add mask and hop limit) and room in its table, and none of that is answered: a node it does not add comes back as `NEW_ADVERT`, silently off the radio.
- A contact's kept advert carries the path it came by, so an exported link may have one.
- A `contact/add` code is not signed: anyone can write one for any key under any name.

## Decision

- **Both forms are read**: the signed advert link and the `contact/add` code. The advert's signature is checked on the phone, before anything is shown; a link whose signature does not verify is refused with its own error, not as an unreadable code, so that the apps can say it was changed or made up.
- **Import first, then add**: a signed advert goes to the radio with `IMPORT_CONTACT`, so that the radio keeps it (and its own owner can share the contact again from the stock app). lichen then asks for the contact by key. If the radio did not take it (manual add, the hop limit, a full table), lichen adds it with `ADD_UPDATE_CONTACT`, from the advert's own fields. A `contact/add` code goes with `ADD_UPDATE_CONTACT` directly. Asking by key after the command, rather than predicting the radio's choice from the settings read at connection, also covers an overwrite of the oldest contact and settings changed from another app.
- **A path is read and kept**: the advert is imported as the link holds it, path included, and the link reports the path (hops and hashes). The radio caches an advert's path (`MyMesh.cpp` L358–L388, read back by `GET_ADVERT_PATH`), so it then knows what the sharer's radio knew, as when the stock app imports a link; an emptied path would make it record the node as heard directly, which is false. The path is not signed and lichen never makes it the contact's own path (the firmware does not either: an advert leaves `out_path` unknown). A path of as many hops as the radio's auto-add limit or more is not added by the import; the fallback below adds it.
- **Only the exported form**: header `0x11` (flood, ADVERT, version 0), as `EXPORT_CONTACT` writes it. `app_data` longer than 32 bytes is refused, although the firmware reads only its first 32: no MeshCore node writes more, and the signature would cover bytes the radio never keeps.
- **Already there**: a key the radio already reported is not sent again; the apps open its chat.
- **No URL scheme**: the stock app owns `meshcore://`, and iOS does not define which app opens a scheme two apps declare. The apps read the link pasted or scanned in Add a contact.

## Consequences

- A contact added from a signed advert carries the name, type and position its node signed; one added from a code carries whatever the code says, and the apps say it is unsigned.
- A contact added with `ADD_UPDATE_CONTACT` (a code, or an advert the radio did not take) has no kept advert: the stock app cannot share it from that radio until the node's next advert is heard.
- The radio's refusal comes as a refused setting (§1.1), with no reason: the firmware gives none beyond a full table.
- Rejected:
  - `ADD_UPDATE_CONTACT` for every link (the radio would never keep the signed advert);
  - trusting `IMPORT_CONTACT`'s `OK` (a radio in manual add answers it and adds nothing);
  - choosing the command from the auto-add settings read at connection (they may have changed since, and a full table is not in them);
  - importing the advert with its path emptied (first version of this ADR): the radio would record the node as heard directly.
