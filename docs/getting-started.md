# Getting started

*[Version française](getting-started.fr.md)*

lichen sends end-to-end encrypted messages from phone to phone over LoRa radios, with no cell network and no Internet. Each phone talks to its own small radio over Bluetooth; the radios relay the messages, through other people's MeshCore repeaters when they are out of reach of each other. This page takes you from nothing to a first message. You need a phone, a radio, and someone else with the same.

## 1. Get a radio

lichen works with an unmodified **MeshCore companion radio**: any LoRa board that runs MeshCore's companion firmware (version 1.18 or later). It has been tested with a **Heltec V4** and a **ThinkNode M5**; other boards the MeshCore flasher lists should work the same way. Pick one for your region's band (868 MHz in Europe, 915 MHz in North America), with its antenna: never power a LoRa radio without its antenna.

A board with a screen shows its pairing code; one with a battery keeps working in a pocket.

## 2. Flash the stock MeshCore companion firmware

lichen never asks you to modify the radio: it uses MeshCore's own firmware.

1. Open MeshCore's web flasher, <https://flasher.meshcore.io>, in Chrome or Edge on a computer.
2. Choose your board, then the **Companion (Bluetooth)** firmware, the latest version.
3. Connect the radio by USB, follow the flasher's steps, and let the radio restart.

A radio that already runs a MeshCore Bluetooth companion firmware, version 1.18 or later, needs nothing.

## 3. Install lichen

- **Android** (12 or later): download [`lichen.apk`](https://github.com/tduh42/lichen/releases/latest/download/lichen.apk) from the latest release, open it and allow the installation.
- **iPhone**: lichen is not on the App Store. With a Mac and a free Apple ID, `scripts/install-ios.sh` builds it and installs it on your iPhone; the README's "On your own iPhone" section has the steps.

## 4. Pair the radio

1. Switch the radio on and keep it next to the phone.
2. Open lichen: **Connect my radio**. The radios around appear; tap **Pair** on yours.
3. The phone asks for a six-digit code: type the one on the radio's screen. A radio without a screen uses `123456` unless someone changed it.
4. lichen then shows your identity: open **Recovery phrase** and write the 24 words on paper. They restore your identity on a new phone; nobody else needs them, ever.

Then check **Settings › Radio settings**: the preset must be the one people around you use. In Europe, most MeshCore communities use **EU/UK (narrow)** (869.618 MHz, 62.5 kHz, SF 8, CR 4/8). A radio on other settings hears nobody. Ask your local MeshCore community when in doubt.

No radio yet? **Try the demo network** (on the pairing screen, or in Settings) runs the real app on a simulated network.

## 5. Add a contact, in person

With the other person next to you, both radios on:

1. In **Messages**, tap the QR code button (**Add a contact**).
2. One of you shows **My code**, the other taps **Scan** and points the camera at it. Then swap.

Scanning in person makes the contact **verified**: lichen knows it is talking to their phone and nobody in between. You can also write to anyone your radio has heard, but their key shows "to verify" until you compare safety numbers or scan their code.

## 6. Send the first message

Open the contact and write. The message is encrypted on your phone, cut into radio packets, sent by your radio, and put back together and decrypted on theirs. When it arrives, it shows as **delivered** with the airtime it used.

Good to know:

- **Airtime:** the radio may only transmit 10 % of the time in this band, 360 seconds an hour. A text costs a fraction of a second; a voice message a few seconds; lichen refuses what does not fit and tells you when it will.
- **Out of reach:** a text waits up to 24 hours for the radio and for the other person, and goes once they are back.
- **People without lichen:** you can still write to people who use the official MeshCore app, and read the Public channel; those chats are MeshCore's own, not lichen-encrypted, and lichen marks them so.

Something did not work? **Settings › About › Report a problem** makes a report without your messages, names or positions, which you can read before sending it.
