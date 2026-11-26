# Testing lichen

Every user-facing flow, numbered, with what it must do. Each scenario is automated at the lowest
layer that can show it:

- **FFI**: `crates/lc-ffi/tests/scenarios_*.rs`, the API on the demo network, as the apps call it
  (runs in the Rust CI job).
- **iOS**: XCUITests in `apps/ios/LichenUITests/`, on the demo network (`scripts/ui-tests-ios.sh`,
  on a Mac, and on every pull request that changes something the iOS app or its tests use: CI
  check `iOS`, the classes shared out over five runners; `scripts/ios-paths.py` says which
  pull requests, and `iOS` passes without the runners for the others).
- **Android**: Compose UI tests in `apps/android/app/src/test/` (Robolectric) or `androidTest/`
  (CI job `android`).

The **Covered by** column is written by `scripts/testing-coverage.py` (`scripts/check.sh` fails
when it is out of date). It lists the tests named after the scenario, one line per suite: FFI
`xx_N_…`, iOS `testXXN_…` and Android `XX_N_…`, as `Class.test` (`Class: test, test` for
several), with `(+fr, dark)` for a test's French and dark-mode variants; tests named otherwise
are listed in the script. It keeps two notes written by hand, after the tests: `real radio: …`,
a part only a real companion shows (the scenario is then in the checklist at the end), and
`not automated: …`, a part no test shows, and why. A scenario needs a test or a note. A bug
found gets a test that fails before its fix, at the lowest layer that shows it.

Launch flags are the debug arguments listed in CLAUDE.md (iOS `-flag value`, Android `--ez`/`--es`/`--ei`). Every iOS UI test on the demo also runs with `-demoSkip 20`, `-demoSpeed 4`,
`-noBanners YES` (unless it looks at notifications) and `-plainHeard YES` (what the demo's
stock-client people send reaches the radio even when the busy air loses it), `LichenUITest.launch`.

**Coverage:** 86 of 88 scenarios have at least one automated test (FFI 77, iOS 82, Android 83, lc-core 7). Written by `python3 -I scripts/testing-coverage.py`.

## 1. Onboarding and identity

| Id | Scenario | Expected | Flags | Covered by |
|----|----------|----------|-------|------------|
| ON-1 | First launch, onboarding to the end | Identity created, recovery phrase shown once, Messages empty with the radio prompt | `-onboarding YES` | FFI on_1_first_launch_creates_an_identity<br>iOS OnboardingTests.testON1_FirstLaunchToTheEnd<br>Android OnboardingTest.ON_1_onboardingToTheEnd (+fr, fr dark) |
| ON-2 | Restart after onboarding | Same identity (fingerprint unchanged), no onboarding | | FFI on_2_restart_keeps_the_identity<br>iOS OnboardingTests.testON2_RestartKeepsTheIdentity<br>Android OnboardingTest.ON_2_restartKeepsTheIdentity |
| ON-3 | Recovery phrase: show, confirm | Words match the identity; wrong confirmation refused | | FFI on_3_recovery_phrase_matches_and_a_wrong_one_is_refused<br>iOS OnboardingTests: testON3_ConfirmTheWrittenPhrase, testON3_ClosingTheCheckIsNotABackup, testON3_RecoveryPhraseRestoresTheSameIdentity, testON3_TheDemoDoesNotBackUpTheRealIdentity<br>Android OnboardingTest: ON_3_recoveryPhraseRestoresItsIdentity (+fr), ON_3_writtenDownPhraseIsChecked (+fr), ON_3_theDemoDoesNotBackUpTheRealIdentity |
| ON-4 | My card / QR | Card shows name and QR; the QR decodes to the card | `-screen identity` | FFI on_4_my_card_and_its_qr<br>iOS OnboardingTests: testON4_MyCodeDecodesToMyCard, testON4_OwnCodeIsSaidSo<br>Android OnboardingTest.ON_4_myCardAndItsQrCode |
| ON-5 | Identity failure screens | Each problem shows its screen and its way out | `-identityProblem …` | FFI on_5_identity_and_database_problems<br>iOS OnboardingTests.testON5_IdentityFailureScreens<br>Android OnboardingTest: ON_5_identityProblemScreens (+fr), ON_5_corruptIdentityStartsANewOne |
| ON-6 | Verify a contact (safety number, scan) | Contact marked verified; number identical on both sides | `-screen verify` | FFI on_6_verify_a_contact<br>iOS OnboardingTests: testON6_VerifyAContact, testON6_TheOtherPhoneIsAskedToCompare, testON6_CompareLaterAndAfterARestart, testON6_CompactBannerAtLargeText, testON6_CompactBannerWithTheKeyboardUp<br>Android OnboardingTest: ON_6_verifyAContact (+fr, dark), ON_6_otherPhoneIsAskedToCompare (+fr), ON_6_compactCompareBannerAtLargeText (+fr), ON_6_compareBannerWithItsNote, ON_6_compactCompareBannerWithTheKeyboardUp (+fr), ON_6_quoteOriginalWithTheKeyboardUp_fr, ON_6_compareLaterAndAfterARestart |
| ON-7 | Add a contact by card (paste, scan) | Contact added, conversation can start | `-screen contact` | FFI on_7_add_a_contact_by_card, on_7_a_card_one_way_can_be_written_to_at_once<br>iOS OnboardingTests: testON7_AddAContactByPastingTheirCode, testON7_AddAContactByScanningTheirCode<br>Android OnboardingTest: ON_7_addContactByCard (+fr), ON_7_scannedInPersonThenShowYourCode (+fr) |

## 2. Radio link and radio settings

| Id | Scenario | Expected | Flags | Covered by |
|----|----------|----------|-------|------------|
| RL-1 | Demo radio connects | "ThinkNode M5 connected", contacts and channels arrive | `-demo YES` | FFI rl_1_demo_radio_connects<br>iOS RadioLinkTests: testRL1_DemoRadioConnects, testRL1_TheDemoRunsFasterWithDemoSpeed<br>Android RadioLinkTest.RL_1_demoRadioConnects (+fr, fr dark)<br>· real radio: pairing over Bluetooth |
| RL-2 | Radio drops, comes back | Banner "Radio disconnected"; waiting texts go once it is back | `-offline 60 -online 150` | FFI rl_2_radio_drops_and_comes_back<br>iOS RadioLinkTests.testRL2_RadioDropsAndComesBack<br>Android RadioLinkTest: RL_2_radioDropsAndComesBack (+fr), RL_2_bannerConnectsTheDemoRadioAgain, RL_2_pairedRadioDropsSaysDisconnected<br>· real radio: reconnecting after the radio restarts, after the phone sleeps |
| RL-3 | Text expires while offline | Marked failed after the expiry, can be resent | `-expiry 30` | FFI rl_3_a_text_expires_while_offline<br>iOS RadioLinkTests.testRL3_TextExpiresWhileOffline<br>Android RadioLinkTest.RL_3_textExpiresWhileOffline (+fr) |
| RL-4 | Saved radios: list, forget, reconnect | The list matches; forgetting removes it | | FFI rl_4_saved_radios<br>iOS RadioLinkTests.testRL4_SavedRadios<br>Android RadioLinkTest.RL_4_savedRadiosListForgetReconnect (+fr) |
| RL-5 | Radio settings: read, change, read back | Values shown match the radio; a change is applied and shown; the radio on top (link, battery, firmware, airtime); the search lands on a setting ("power", "puissance": Transmit power) | `-screen radio` | FFI rl_5_radio_settings_read_change_read_back, rl_5_a_slow_radio_s_settings<br>iOS RadioLinkTests: testRL5_RadioSettingsReadChangeReadBack, testRL5_SettingsBeingRead, testRL5_GpsWithoutFixTakesThePhonesPosition, testRL5_PhonePositionOfferedWithoutTheMap, testRL5_SearchLandsOnTheSetting<br>Android RadioLinkTest: RL_5_radioSettingsReadAndChanged (+fr dark), RL_5_radioHeader (+fr dark), RL_5_searchRadioSettings (+fr), RL_5_advertisedPositionInFigures (+fr), RL_5_gpsWithoutFixTakesThePhonesPosition (+fr), RL_5_settingsBeingRead (+fr), RL_5_meshcorePublicKeyAndCode (+fr)<br>· real radio: a change seen in the stock MeshCore app |
| RL-6 | Refused setting | Error shown, the old value stays | `-screen radio` | FFI rl_6_a_refused_setting, rl_6_a_setting_the_radio_refuses<br>iOS RadioLinkTests: testRL6_RefusedSetting, testRL6_TheRadioRefusesASetting<br>Android RadioLinkTest: RL_6_refusedSettingKeepsTheOldValue (+fr), RL_6_radioRefusesASetting (+fr) |
| RL-7 | Battery and radio card | Battery level shown; radio details when turned on; the card names the board and firmware (the demo: Elecrow ThinkNode M5, v1.18.0) and the radio's noise floor, uptime and packets, read again at most every 60 s | `-lichen.radioDetails YES` | FFI rl_7_the_radio_card_s_board_and_figures, rl_7_battery_and_radio_details<br>iOS RadioLinkTests.testRL7_BatteryAndRadioDetails<br>Android RadioLinkTest.RL_7_batteryAndRadioCard (+fr)<br>lc-core tests/radio.rs: the_radio_s_board_and_figures |
| RL-8 | Radio clock set at connection | The radio's time follows the phone's | | lc-core tests/radio.rs: connecting_sets_the_radios_clock |
| RL-9 | Contact list read whole / cut short | All contacts, asked again when records are missing | | lc-core src/core.rs: a_contact_list_cut_short_still_counts, records_lost_before_the_end_are_asked_for_again<br>· real radio: every contact of a real companion |
| RL-10 | Send an advert | A choice, nearby or the whole mesh, each saying what it reaches; what it carries (the name, the position when advertised); the time and reach once the radio answered; not sent without a radio | | iOS RadioLinkTests: testRL10_SendAnAdvert, testRL10_AdvertCarriesThePosition<br>Android RadioLinkTest.RL_10_sendAnAdvert (+fr) |
| RL-11 | Each radio setting says what happened | On its row: the value asked with a spinner while it waits; then "Saved on the radio", or the radio kept its old value, refused it, or did not answer, the radio's value shown | `-screen radio -radioIgnores preferences`, `-radioRefuses power`, `-radioSlow 20` | FFI rl_11_each_setting_says_what_happened<br>iOS RadioLinkTests: testRL11_ASettingSavedOnTheRadio, testRL11_ASettingGoesOutOnAQuietRadio, testRL11_ASettingTheRadioDoesNotKeep<br>Android RadioLinkTest: RL_11_eachSettingSaysWhatHappened (+fr), RL_11_aSettingGoesOutOnAQuietRadio (+fr), RL_11_aSettingTheRadioDoesNotKeep (+fr) |
| RL-12 | The radio line names the reading after a connection | While a connection fills its lists, the radio line says the step: reading the radio, contacts n of the count the radio announced, channels, waiting messages fetched; then nothing; a radio switched off while a message comes gives it as a waiting message at the next connection | `-manyContacts 200` | FFI rl_12_the_reading_after_a_connection<br>lc-core src/plain.rs: the_reading_goes_through_its_steps, a_disconnection_clears_the_reading |

## 3. Contacts, nodes, rooms, repeaters, requests, blocks

| Id | Scenario | Expected | Flags | Covered by |
|----|----------|----------|-------|------------|
| CT-1 | Contacts list | People only, sorted, names and hop badges right | | FFI ct_1_contacts_list<br>iOS ContactsTests.testCT1_ContactsList<br>Android ContactsTest: CT_1_contactsList (+fr), CT_1_contactsSortedAsPeopleReadThem<br>· real radio: every person a real companion holds |
| CT-2 | Rename a contact | Shown everywhere (list, chat title, notifications); reset restores the node name | | FFI ct_2_rename_a_contact<br>iOS ContactsTests.testCT2_RenameAContact<br>Android ContactsTest: CT_2_renameAContact (+fr), CT_2_contactsMeshcorePublicKey (+fr) |
| CT-3 | The Nodes tab: repeaters, rooms, heard nodes | Opens on the node list, each with its kind, position, last heard; a node's page pushes from it; Settings no longer lists them; the nodes heard directly with their last signal, rated Good (hub), Fair (Lucie) or Bad (Refuge board), a node behind a repeater (crest) with none; each reading kept in its history, the last one back after a restart; the list in two sections, in reach (by rating, a signal meter each) and through repeaters (by hops, a hop pill each), filters with counts and Heard in 2 h; a node's page with its large meter and its signal over 24 h or 7 days, the chart's Good and Fair bands at the preset's limits, given by the core (none before a radio) | `-tab nodes` | FFI ct_3_nodes, ct_3_node_signal, ct_3_signal_limits<br>iOS ContactsTests: testCT3_NodesTabOpensOnTheList, testCT3_Nodes, testCT3_LichenUserInTheNodes, testCT3_HubPage<br>Android ContactsTest: CT_3_nodes (+fr dark), CT_3_lichenUserInTheNodes, CT_3_nodeFilters (+fr), CT_3_hubPage (+fr), CT_3_nodesTabOpensOnTheList (+fr), CT_3_nodeMeshcorePublicKey (+fr)<br>lc-core tests/signal.rs: a_direct_advert_is_attributed_and_a_relayed_one_is_not, a_one_byte_hash_is_attributed_only_when_unique, ratings_match_the_vectors, the_limits_are_where_the_ratings_change, the_history_is_capped_by_count_and_by_age; tests/search.rs: group_envelopes_stay_with_locate |
| CT-4 | Room: log in, read posts, post | Posts arrive, the room's row is in Messages, a post goes out | `-roomLogin hello -open "Refuge board"` | FFI ct_4_room_log_in_read_and_post<br>iOS ContactsTests.testCT4_RoomLoginReadAndPost<br>Android ContactsTest.CT_4_roomLogInReadPost (+fr) |
| CT-5 | Room: wrong password | Refused, said so | | FFI ct_5_room_wrong_password<br>iOS ContactsTests.testCT5_RoomWrongPassword<br>Android ContactsTest.CT_5_roomWrongPassword |
| CT-6 | Repeater: admin login, status, CLI | Status read, a CLI command answered | | FFI ct_6_repeater_admin_status_and_cli<br>iOS ContactsTests.testCT6_RepeaterAdmin<br>Android ContactsTest.CT_6_repeaterAdmin (+fr) |
| CT-7 | Trace to a node | Hops and SNRs shown; the route to crest goes You → hub → crest with each stop's position, the hub leg measured from its adverts, the crest leg once traced, an unmeasured leg shown as such; on the crest's page, the route ribbon ("not measured" until Trace again measures it), no signal of its own, Show on the map | | FFI ct_7_trace_to_a_node, ct_7_route_to_a_node<br>iOS ContactsTests.testCT7_TraceToANode<br>Android ContactsTest.CT_7_traceToANode (+fr)<br>lc-core tests/signal.rs: the_route_joins_the_path_the_readings_and_the_trace |
| CT-14 | A node's readings | Logged in to crest as a guest, Ask gives its battery (3.92 V, about 69 %) and temperature (4 °C) with when they came, again when asked again, one request at a time; Marc never answers: "no answer"; Lucie, not on the radio, cannot be asked; on a node's page, the card Readings it sends: nothing asked by opening it, Log in first for a repeater (to its login card), each row read as one, the battery's line over 7 days from two readings | | FFI ct_14_a_node_s_readings<br>Android ContactsTest: CT_14_crestReadings (+fr, dark), CT_14_marcNeverAnswers (+fr), CT_14_lucieIsNotAContact (+fr)<br>lc-core tests/readings.rs: a_request_answered, a_request_timed_out, two_requests_to_one_node_give_one_in_flight, the_history_is_capped_by_age_and_count |
| CT-8 | Introduction request: accept | Request card in Messages; accept opens the chat with its messages | `-requestDemo YES` | FFI ct_8_introduction_request_accept<br>iOS ContactsTests.testCT8_RequestAccepted<br>Android ContactsTest.CT_8_requestAccepted (+fr) |
| CT-9 | Request: block, delete | Blocked: nothing more from them; deleted: gone | `-requestDemo YES` | FFI ct_9_request_block_and_delete<br>iOS ContactsTests: testCT9_RequestBlocked, testCT9_RequestDeleted<br>Android ContactsTest: CT_9_requestBlockedThenDeleted, CT_9_requestDeleted |
| CT-10 | Block and unblock a contact | Their messages stop and come back; Settings › Blocked lists them | | FFI ct_10_block_and_unblock_a_contact, ct_10_our_text_on_the_air_at_the_block<br>iOS ContactsTests.testCT10_BlockAndUnblock<br>Android ContactsTest.CT_10_blockAndUnblock (+fr) |
| CT-11 | A contact the radio files as a non-person | Their conversation stays in Messages under their name | `-marcRepeater YES` | FFI ct_11_a_person_the_radio_files_as_a_repeater<br>iOS ContactsTests.testCT11_PersonFiledAsARepeaterKeepsTheirRow<br>Android ContactsTest.CT_11_contactFiledAsNonPerson |
| CT-12 | Add a MeshCore contact from a link (paste, scan) | The stock app's signed link or a contact code adds the node to the radio: a person among Contacts, a repeater or room among the nodes; one already there opens its chat; a changed link is refused | | FFI ct_12_add_a_meshcore_contact_from_a_link<br>iOS ContactsTests: testCT12_AddAMeshCoreContactFromASignedLink, testCT12_AChangedLinkIsRefused, testCT12_AContactCodeIsAddedUnsigned, testCT12_ALinkWithNoRadio<br>Android ContactsTest: CT_12_addFromASignedLink (+fr), CT_12_changedLinkRefused (+fr), CT_12_contactCodeAddedUnsigned (+fr), CT_12_linkWithNoRadio |
| CT-13 | New conversation when the radio knows many people | New group, Join a channel, Add a contact and the channels (Public first) are on the first screen, the people last; the search finds people, channels and groups; `#tag` joins that hashtag channel and opens it (the channel itself once followed); the people filter (All, lichen, MeshCore, with counts) shows only those; New group is its own screen (name, lichen contacts with a search, Create) | `-manyContacts 200` | iOS ContactsTests: testCT13_NewConversationWithManyPeople, testCT13_HashtagJoinsAndOpens, testCT13_NewGroupScreen<br>Android ContactsTest: CT_13_newConversationWithManyPeople (+fr, fr dark, dark), CT_13_hashtagJoinsAndOpens (+fr), CT_13_newGroupScreen (+fr) |

## 4. Direct lichen chats

| Id | Scenario | Expected | Flags | Covered by |
|----|----------|----------|-------|------------|
| DM-1 | Send a text | Sent, then delivered; shown once | `-open Camille -say …` | FFI dm_1_send_a_text<br>iOS DirectChatTests.testDM1_SendAText<br>Android DirectChatTest.DM_1_sendAText (+fr dark)<br>· real radio: to another lichen user, with its delivered mark |
| DM-2 | Receive a text | Row updates, unread badge, notification | | FFI dm_2_receive_a_text<br>iOS DirectChatTests: testDM2_ReceiveAText, testDM2_AFirstMessageShowsInAnOpenChat<br>Android DirectChatTest.DM_2_receiveAText (+fr)<br>· real radio: from another lichen user |
| DM-3 | Long text (fragmented) | Arrives whole | | FFI dm_3_long_text<br>iOS DirectChatTests.testDM3_LongText<br>Android DirectChatTest: DM_3_longTextArrivesWhole, DM_3_longTextMenuWithTheKeyboardUp_fr |
| DM-4 | Reply | Quoted message shown and tappable | `-reply …` | FFI dm_4_reply_quotes_the_message<br>iOS DirectChatTests.testDM4_Reply<br>Android DirectChatTest: DM_4_reply (+fr), DM_4_quoteOriginalWithTheKeyboardUp_fr |
| DM-5 | React, change, remove reaction | Shown on both sides | `-react …` | FFI dm_5_react_change_and_remove<br>iOS DirectChatTests.testDM5_Reactions<br>Android DirectChatTest.DM_5_reactChangeRemove |
| DM-6 | Edit a sent message | Edited text and mark on both sides | | FFI dm_6_edit_a_sent_message<br>iOS DirectChatTests.testDM6_EditASentMessage<br>Android DirectChatTest.DM_6_editASentText (+fr) |
| DM-7 | Delete for me / for everyone | Gone here; for everyone gone there too | | FFI dm_7_delete_for_me_and_for_everyone<br>iOS DirectChatTests.testDM7_DeleteForMeAndForEveryone<br>Android DirectChatTest.DM_7_deleteForMeAndForEveryone (+fr) |
| DM-8 | Disappearing timer | Messages vanish after the time, both sides | `-timer 300` | FFI dm_8_disappearing_timer<br>iOS DirectChatTests.testDM8_DisappearingTimer<br>Android DirectChatTest: DM_8_disappearingTimerIsSet, DM_8_messagesVanishAfterTheTimer |
| DM-9 | Paging older messages | Older pages load, order kept, no duplicates | `-fill 250 -earlier YES` | FFI dm_9_paging_older_messages<br>iOS DirectChatTests.testDM9_PagingOlderMessages; ChatNavigationTests.testALongChatKeepsItsPlaces<br>Android DirectChatTest.DM_9_pagingOlderMessages |
| DM-10 | Search messages | Results across chats; opening one shows it in place | `-screen searchMessages -query …` | FFI dm_10_search_messages<br>iOS DirectChatTests.testDM10_SearchMessages<br>Android DirectChatTest: DM_10_searchMessages (+fr), DM_10_shortChatStaysOnItsResult (+fr) |
| DM-11 | Message Info | Times, route, fragments, airtime | `-screen info` | FFI dm_11_message_info<br>iOS DirectChatTests.testDM11_MessageInfo<br>Android DirectChatTest.DM_11_messageInfo (+fr) |
| DM-12 | Unread counts and mark read | Badge per row and on the tab; opening clears | | FFI dm_12_unread_counts_and_mark_read<br>iOS DirectChatTests.testDM12_UnreadCountsAndMarkRead<br>Android DirectChatTest: DM_12_longChatHoldsItsPlaceWhenAMessageArrives (+fr), DM_12_unreadAndMarkRead (+fr) |
| DM-13 | Forward a message | Arrives in the other chat | | FFI dm_13_forward_a_message<br>iOS DirectChatTests: testDM13_ForwardAMessage, testDM13_ForwardLeavesBlockedContactsOut<br>Android DirectChatTest: DM_13_forward, DM_13_forwardLeavesBlockedContactsOut |
| DM-14 | Copy, links and places in a text | Links open; places open the map | | FFI dm_14_links_and_places_in_a_text<br>iOS DirectChatTests.testDM14_CopyLinksAndPlaces<br>Android DirectChatTest.DM_14_linksAndPlaces |

## 5. MeshCore chats and channels

| Id | Scenario | Expected | Flags | Covered by |
|----|----------|----------|-------|------------|
| MC-1 | Plain chat with a stock MeshCore user | Marc's message arrives; the answer reaches him; badge "MeshCore" | `-open Marc -sayPlain …` | FFI mc_1_marcs_message_on_a_radio_reached_over_tcp, mc_1_plain_chat_with_marc<br>iOS MeshCoreTests.testMC1_PlainChatWithAStockUser<br>Android MeshCoreTest.MC_1_plainChatWithMarc (+fr dark)<br>· real radio: with a stock MeshCore app user |
| MC-2 | Public channel: read and post | Posts with their authors; a post goes out | `-open Public` | FFI mc_2_public_channel_read_and_post<br>iOS MeshCoreTests.testMC2_PublicChannelReadAndPost<br>Android MeshCoreTest.MC_2_publicChannelReadAndPost (+fr)<br>· real radio: a Public post |
| MC-3 | Channel QR: show and join | The code shows after the warning; scanning one joins the channel | | FFI mc_3_channel_qr_show_and_join<br>iOS MeshCoreTests.testMC3_ChannelCodeShowAndJoin<br>Android MeshCoreTest.MC_3_channelQrShowAndJoin (+fr) |
| MC-4 | Import the MeshCore app's history | Messages imported once; a second import adds nothing | | FFI mc_4_import_the_meshcore_app_history<br>iOS MeshCoreTests.testMC4_ImportTheMeshCoreHistory<br>Android MeshCoreTest.MC_4_importTheMeshCoreHistory (+fr) |
| MC-5 | Mentions and replies | `@[Name]` shown as a mention; a reply quotes that person's latest earlier message; a mention of our radio is highlighted and notifies even when muted; Reply sends `@[Name] ` first | `-open Public` | FFI mc_5_mentions_and_replies<br>iOS MeshCoreTests.testMC5_MentionsAndReplies<br>Android MeshCoreTest.MC_5_mentionsAndReplies (+fr) |

## 6. Media

| Id | Scenario | Expected | Flags | Covered by |
|----|----------|----------|-------|------------|
| MD-1 | Send a photo | Preview goes; cost shown first; refused when over the hour's airtime | `-screen media` | FFI md_1_send_a_photo<br>iOS MediaTests.testMD1_SendAPhotoAndTheFullOneOnRequest<br>Android MediaTest: MD_1_sendAPhoto (+fr), MD_1_photoRefusedOverTheHoursAirtime<br>· real radio: a photo over the air |
| MD-2 | Full photo requested | Card on the photo; Send sends it, Not now doesn't; asking again shows it again | `-media YES` | FFI md_2_full_photo_requested<br>iOS MediaTests.testMD2_FullPhotoNotNowThenAskedAgain<br>Android MediaTest.MD_2_fullPhotoRequested<br>· real radio: the full variant over the air |
| MD-3 | Receive a photo, open the viewer | Thumbnail, viewer, save | `-screen photo` | FFI md_3_receive_a_photo<br>iOS MediaTests.testMD3_ReceiveAPhotoAndOpenTheViewer<br>Android MediaTest: MD_3_receiveAPhotoAndOpenIt (+fr dark), MD_3_saveToTheGallery (+fr) |
| MD-4 | Voice: record, review, send, play | Plays on both sides | `-screen voiceReview` | FFI md_4_voice_record_send_play<br>iOS MediaTests.testMD4_VoiceReviewSendPlay<br>Android MediaTest.MD_4_voiceReviewSendPlay (+fr) |
| MD-5 | Files: small note, big document | Small sent; big refused or offered by bridge | `-screen file` / `fileBig` | FFI md_5_files_small_note_and_big_document<br>iOS MediaTests: testMD5_SmallFileSent, testMD5_BigFileNotOverLoRa<br>Android MediaTest: MD_5_smallFileSent (+fr), MD_5_bigFileRefusedOverLora (+fr) |
| MD-6 | GIFs and stickers | Picker, send, shown animated | `-sticker …` | FFI md_6_gifs_and_stickers<br>iOS MediaTests: testMD6_StickerSentAndAnswered, testMD6_GifFromAGiphyLink<br>Android MediaTest.MD_6_stickerAndGif<br>· not automated: the GIF loaded from GIPHY and shown animated (Internet; by hand: `cargo test -p lc-bridge --release -- --ignored gif`) |
| MD-7 | Bridge: fetch a photo over iroh | Fetched, checked, shown | `-media YES` | not automated: needs the Internet; by hand: `cargo test -p lc-ffi --release -- --ignored bridge` |

## 7. Groups

| Id | Scenario | Expected | Flags | Covered by |
|----|----------|----------|-------|------------|
| GR-1 | Create a group | Shown in Messages, alone in it | | FFI gr_1_create_a_group<br>iOS GroupTests.testGR1_CreateAGroup<br>Android GroupTest.GR_1_createAGroup (+fr dark) |
| GR-2 | Invite by QR, someone joins | The member appears; their posts arrive | `-screen invite -inviteDemo YES` | FFI gr_2_invite_by_qr_someone_joins<br>iOS GroupTests.testGR2_InviteByQRSomeoneJoins<br>Android GroupTest.GR_2_inviteByQrSomeoneJoins (+fr) |
| GR-3 | Join by scanning an invitation | Group opens with its members | `-screen join -joinNow YES` | FFI gr_3_join_by_scanning_an_invitation<br>iOS GroupTests.testGR3_JoinByScanningAnInvitation<br>Android GroupTest.GR_3_joinByScanningAnInvitation (+fr) |
| GR-4 | Invitation received, accept / decline | Card in Messages; accepted joins, declined goes | | FFI gr_4_invitation_received_accept_and_decline<br>iOS GroupTests: testGR4_InvitationAccepted, testGR4_InvitationDeclined<br>Android GroupTest: GR_4_invitationAccepted, GR_4_invitationDeclined_fr |
| GR-5 | Post and receive in a group | Authors named, order kept | | FFI gr_5_post_and_receive_in_a_group<br>iOS GroupTests.testGR5_PostAndReceive<br>Android GroupTest.GR_5_postAndReceiveInAGroup (+fr) |
| GR-6 | A post lost, recovered late | Gap revealed, post comes back marked late | `-demo YES` (Julien) | FFI gr_6_a_post_lost_recovered_late<br>iOS GroupTests.testGR6_ALostPostComesBackLate<br>Android GroupTest.GR_6_lostPostComesBackLate (+fr) |
| GR-7 | Group voice note (≤ 5 fragments) | Cost shown, sent, plays | | FFI gr_7_group_voice_note<br>iOS GroupTests.testGR7_GroupVoiceNote<br>Android GroupTest.GR_7_groupVoiceNote (+fr) |
| GR-8 | Rename, members, leave | Changes shown to all; after leaving, nothing more arrives | | FFI gr_8_rename_members_and_leave<br>iOS GroupTests.testGR8_RenameMembersLeave<br>Android GroupTest.GR_8_renameMembersLeave (+fr) |

## 8. Location, map, Locate and SOS

| Id | Scenario | Expected | Flags | Covered by |
|----|----------|----------|-------|------------|
| MP-1 | Share my position with a contact | Precision chosen; their side sees it; stopping ends it | `-share Camille` | FFI mp_1_share_my_position_with_a_contact<br>iOS LocationTests.testMP1_ShareMyPositionWithAContact<br>Android LocationTest: MP_1_shareMyPositionWithAContact (+fr), MP_1_shareExtra<br>· not automated: what the other side sees of the position (the demo's people show nothing of it) |
| MP-2 | Positions received on the map | Camille exact, Julien coarse (1 km cell) | | FFI mp_2_positions_received_on_the_map<br>iOS LocationTests.testMP2_PositionsReceivedOnTheMap<br>Android LocationTest.MP_2_positionsReceivedOnTheMap (+fr dark) |
| MP-3 | Nodes layer, a node's route | Repeaters and rooms where they advertise, a ring in its rating's colour around a node heard directly; a node's route (from its page's Show on the map, or the node tapped): a line per leg in its rating's colour with its SNR, dashed where nothing measured it, stops without a position skipped; its card (hops, via, traced, the legend, or no positions to draw it), closed; the legend in the layers menu | | FFI mp_3_nodes_layer<br>iOS LocationTests: testMP3_NodesLayer, testMP3_RouteOnTheMap<br>Android LocationTest: MP_3_nodesLayer (+fr), MP_3_routeOnTheMap (+fr) |
| MP-4 | Offline maps | Download a region; the map works without network | | iOS LocationTests.testMP4_OfflineMapsOffered<br>Android LocationTest.MP_4_offlineMapsOffered (+fr)<br>· not automated: downloading a region, the map without network (Internet) |
| MP-5 | SOS: send, receive, stop | Beacon shown to the group; stop ends it; the SOS button in both of the Map's modes | `-sosdemo YES` | FFI mp_5_sos_send_receive_stop<br>iOS LocationTests: testMP5_SosReceivedSentStopped, testMP5_SosFromLocate<br>Android LocationTest: MP_5_sendAndStopAnSos (+fr dark), MP_5_receiveSos (+fr), MP_5_sosFromLocateMode (+fr) |
| MP-6 | Locate a member without GPS | The Map's Locate mode shows the search; it converges, estimate shown; an SOS counts on the Map tab | `-sosdemo YES -tab locate` | FFI mp_6_locate_a_member_without_gps<br>iOS LocationTests: testMP6_LocateIsAModeOfTheMap, testMP6_LocateAMemberWithoutGps<br>Android LocationTest: MP_6_locateIsAModeOfTheMap (+fr dark), MP_6_locateAMemberWithoutGps (+fr) |

## 9. Settings

| Id | Scenario | Expected | Flags | Covered by |
|----|----------|----------|-------|------------|
| ST-1 | Privacy settings | Each switch applied and kept | `-screen privacy` | FFI st_1_privacy_settings_applied_and_kept<br>iOS SettingsTests.testST1_PrivacySettingKept<br>Android SettingsTest.ST_1_privacySettings (+fr) |
| ST-2 | App lock | Cover at launch and after the delay; unlock with Face ID / passcode | `-lichen.lock YES` | iOS SettingsTests.testST2_AppLockCoversTheApp<br>Android SettingsTest.ST_2_appLock (+fr dark)<br>· not automated: unlocking with Face ID (iOS) |
| ST-3 | Backup: export, restore | The restored phone has contacts, groups, messages | | FFI st_3_backup_export_and_restore<br>iOS SettingsTests: testST3_BackupExport, testST3_BackupRestored<br>Android SettingsTest.ST_3_backupExportAndRestore<br>· not automated: the system's file picker itself (iOS stands in for it) |
| ST-4 | Problem report | Built, shareable, no names or keys in it | | FFI st_4_problem_report_carries_no_secret<br>iOS SettingsTests.testST4_ProblemReport<br>Android SettingsTest.ST_4_problemReport |
| ST-5 | About and version | Version and build of the release | | iOS SettingsTests.testST5_Version<br>Android SettingsTest.ST_5_aboutAndVersion |
| ST-6 | French throughout | No English left on any screen | Android `cmd locale …` | iOS SettingsTests.testST6_FiguresOfTheLanguage; StringsTests.testD6_NoEnglishWordsLeftInFrench<br>Android SettingsTest: ST_6_frenchThroughout, ST_6_frenchFigures_fr |
| ST-7 | Dark mode throughout | Every screen readable | Android `cmd uimode night yes` | not automated: readability is judged by eye (Android's dark variants and `scripts/ui-tests-ios.sh --dark` run the screens in dark) |
| ST-8 | Bridge settings (iroh, Tor, relay only) | Applied; Tor starts and stops | `-lichen.tor YES` | FFI st_8_bridge_settings<br>iOS SettingsTests.testST8_BridgeSettings<br>Android SettingsTest.ST_8_bridgeSettings (+fr)<br>· not automated: Tor reaching "reachable" (Internet; by hand: `cargo test -p lc-bridge --release -- --ignored tor`) |
| ST-9 | Accessibility | Every screen passes the audit | | iOS AccessibilityAuditTests<br>Android SettingsTest.ST_9_accessibility |

## 10. Lifecycle

| Id | Scenario | Expected | Flags | Covered by |
|----|----------|----------|-------|------------|
| LC-1 | Notification opens the right chat | Tapping it opens that conversation (each kind: lichen, MeshCore, channel, room, group) | | FFI lc_1_a_notification_opens_the_right_chat<br>iOS LifecycleTests: testLC1_MeshCoreNotification, testLC1_ChannelNotification, testLC1_RoomNotification, testLC1_GroupNotification; ChatNavigationTests.testTappingANotificationOpensItsChat<br>Android LifecycleTest.LC_1_notificationOpensTheRightChat (+fr) |
| LC-2 | Mute, archive, pin | Muted: no notification; archived: back on a message unless muted; pinned on top | | FFI lc_2_mute_archive_pin<br>iOS LifecycleTests.testLC2_PinArchiveMute<br>Android LifecycleTest.LC_2_muteArchivePin (+fr) |
| LC-3 | Delete a conversation | Gone; a new message starts it again | | FFI lc_3_delete_a_conversation<br>iOS LifecycleTests.testLC3_DeleteAConversation<br>Android LifecycleTest.LC_3_deleteAConversation (+fr) |
| LC-4 | Restart keeps everything | Messages, contacts, groups, invitations, requests, prefs | | FFI lc_4_restart_keeps_everything, lc_4_restart_keeps_requests_and_invitations<br>iOS LifecycleTests.testLC4_RestartKeepsEverything<br>Android LifecycleTest.LC_4_restartKeepsEverything |
| LC-5 | Background and back | Messages received meanwhile are there; no duplicates | | FFI lc_5_background_and_back<br>iOS LifecycleTests.testLC5_BackgroundAndBack<br>Android LifecycleTest.LC_5_backgroundAndBack (+fr) |
| LC-6 | Every conversation listed | Any chat with a message has a row, whatever the contact list says | | FFI lc_6_every_conversation_listed<br>iOS LifecycleTests.testLC6_EveryConversationListed; ContactsTests.testCT11_PersonFiledAsARepeaterKeepsTheirRow<br>Android LifecycleTest.LC_6_everyConversationListed |

## Real radio checklist (on the phone)

What the demo network cannot show, to run on the iPhone with a real companion:

1. Bluetooth: pair a new radio, reconnect after the radio restarts, after the phone sleeps (RL-1, RL-2).
2. Contacts: every person the radio holds is in Contacts (`lichen contacts <port>` gives the radio's list)
   (CT-1, RL-9).
3. A lichen text to and from another lichen user; delivered mark (DM-1, DM-2).
4. A photo, then its full variant on request (MD-1, MD-2).
5. A MeshCore chat with a stock app user; a Public channel post (MC-1, MC-2).
6. Radio settings changed in lichen, seen in the stock app (RL-5).
