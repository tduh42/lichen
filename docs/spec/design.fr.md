# Spécification — Protocole applicatif sur MeshCore

Nov 6, 2026 · @Tanguy

## Contexte et objectifs

Une couche applicative chiffrée au-dessus de MeshCore, qui tourne côté hôte (téléphone ou PC) et n'exige aucune modification du firmware des relais ni des companions. Elle ajoute ce que MeshCore ne fait pas : messages longs, crypto moderne, groupes privés, médias, localisation et recherche de personne.

**Objectifs**

- Fragmentation et réassemblage fiables de datagrammes jusqu'à environ 10 Ko.
- Chiffrement authentifié moderne (AEAD), en remplacement de la crypto MeshCore.
- Groupes privés avec authentification de l'émetteur.
- Images et voix, via la LoRa en repli et via un pont Internet temporaire (iroh ou Tor) quand il est disponible.
- Partage de localisation avec contrôle de la précision par destinataire.
- Mode recherche hors infrastructure : retrouver un membre du groupe avec des ThinkNode M5 (RSSI, caps, SOS).
- Applications iOS (SwiftUI) et Android (Jetpack Compose).

**Non-objectifs**

- Appels vocaux temps réel sur LoRa (duty cycle).
- Compatibilité avec les clients MeshCore officiels pour les nouvelles fonctions : ils ignorent simplement nos paquets.
- Remplacer le routage MeshCore : on l'utilise tel quel.

## Architecture générale

Toute la logique (fragmentation, crypto, groupes, médias, localisation, recherche) vit dans un cœur Rust partagé par les apps iOS et Android. Le companion et les relais MeshCore restent en firmware stock. La seule exception prévue est le mode recherche embarqué sur ThinkNode M5.

&#91;embedded content: architecture · apps, cœur Rust, trois transports\]

Le cœur choisit le transport par ordre de préférence : iroh d'abord, Tor en repli, LoRa quand il n'y a pas d'Internet. Seule la voie LoRa traverse le companion et le réseau MeshCore.

## Contraintes MeshCore

Les relais MeshCore réémettent en direct tout payload sauf quatre types spéciaux, ce qui rend `RAW_CUSTOM` utilisable sans toucher au réseau. Constats tirés du firmware `meshcore-dev/MeshCore`, commit `81dbae5` (15 octobre 2026), fichiers `src/Mesh.cpp`, `src/Packet.cpp` et `examples/companion_radio/MyMesh.cpp`.

**Trame sur l'air** : `header(1) | transport_codes(4, optionnel) | path_len(1) | path(0–64) | payload(0–184)`, 255 octets au total au maximum. Le plafond de 255 octets vient de la radio (champ longueur LoRa sur 1 octet) : on ne peut pas agrandir le MTU, seulement fragmenter.

| Type de payload | Relayé en direct | Relayé en flood | Usage dans ce protocole |
| --- | --- | --- | --- |
| `RAW_CUSTOM` (0x0F) | Oui | Non | Transport unicast de tous nos messages |
| `GRP_DATA` (0x06) | Oui | Oui, même sans déchiffrer | Enveloppe flood pour les groupes |
| Types réservés 0x0C–0x0E | Oui | Non | À éviter : jetés par les extrémités, collision possible avec un futur type |
| `MULTIPART` (0x0A) | Sous-type ACK seulement | Non | À éviter : tout autre sous-type est jeté au premier relais |

**Pièges à respecter**

1. **Dédoublonnage** : le hash anti-doublon porte sur le type et le payload, pas sur le path (`Packet::calculatePacketHash`), avec environ 160 entrées par relais. Un fragment réémis à l'identique est jeté : chaque réémission doit différer d'au moins un octet.
2. **Pas d'adressage dans RAW** : au dernier saut, tous les nœuds à portée remontent le paquet. Destination et source sont dans notre en-tête.
3. **Pas de crypto dans RAW** : tout le chiffrement est à notre charge.
4. **Crypto MeshCore insuffisante** : AES-128 par blocs indépendants (de type ECB) et HMAC tronqué à 2 octets. On ne s'appuie jamais dessus.
5. **Position en clair** : les adverts portent lat/lon en clair et sont floodés. Notre protocole ne met jamais la position dans un advert.

**Interface companion utilisée (firmware stock)**

| Commande | Rôle |
| --- | --- |
| `CMD_SEND_RAW_DATA` (25) | Envoi d'un payload RAW de 4 à 184 octets en direct, path fourni |
| `CMD_SEND_RAW_PACKET` (65) | Injection d'un paquet arbitraire, y compris en flood |
| `PUSH_CODE_RAW_DATA` | Réception d'un RAW, avec SNR et RSSI |
| `PUSH_CODE_LOG_RX_DATA` (0x88) | Toute trame brute reçue, avec SNR et RSSI |
| Option répétition (`isRepeatEn`) | Le companion relaie les paquets des autres |

## Couche fragmentation

Un datagramme est découpé en 64 fragments au plus, d'environ 160 octets utiles chacun, soit environ 10 Ko. Les fragments sont acquittés par bitmap, selon un mode inspiré de SCHC ACK-on-Error (RFC 8724). La fragmentation n'est autorisée qu'en routage direct.

**En-tête commun (7 octets, en tête du payload RAW)**

| Champ | Taille | Contenu |
| --- | --- | --- |
| `dst` | 1 o | Hash de la destination (ou du groupe) |
| `src` | 1 o | Hash de l'émetteur |
| `tag` | 2 o | ID aléatoire du datagramme, unique par paire src/dst |
| `kind + index` | 1 o | Bits 7-6 : type (00 data, 01 ACK, 10 abort, 11 contrôle). Bits 5-0 : index du fragment (0–63) |
| `total` | 1 o | Nombre de fragments (1–64), répété dans chaque fragment |
| `attempt` | 1 o | Numéro de tentative : rend chaque réémission unique pour le dédoublonnage, et entre dans le nonce |

Suivent le texte chiffré du fragment et un tag AEAD de 8 octets. Budget : 184 − 7 − 8 = 169 octets utiles par fragment au maximum.

**ACK (payload chiffré de 9 octets)** : `bitmap(8) | flags(1)`. Chaque bit du bitmap correspond à un fragment reçu.

**Règles**

1. Le récepteur n'envoie l'ACK qu'à la réception du dernier fragment ou à l'expiration d'un timer de réassemblage.
2. L'émetteur ne réémet que les fragments absents du bitmap, avec `attempt` incrémenté. Au-delà d'un nombre maximum de tours, il envoie un abort.
3. Le réassemblage se fait de bout en bout. Les relais sont des relais MeshCore stock et ne gardent aucun état.
4. Le buffer de réassemblage est alloué à la réception du premier fragment, quel que soit son index, grâce au champ `total`.
5. Un fragment n'est accepté qu'après vérification de son tag AEAD : un fragment forgé ne peut pas empoisonner le buffer.
6. Fragmentation interdite en flood : un datagramme de 30 fragments floodé coûterait 30 émissions par relais. Une découverte de chemin (`PATH`) précède le premier envoi.

Ordre de grandeur de la fiabilité sans ACK sélectif : avec 10 % de perte par saut, 10 fragments sur 3 sauts n'arrivent tous qu'environ 4 % du temps (0,9³⁰). D'où le bitmap.

## Cryptographie

Chaque fragment est chiffré séparément en ChaCha20-Poly1305, avec un nonce implicite (0 octet transmis) et un tag tronqué à 8 octets, sous une clé de session dérivée par HKDF.

**Choix de l'algorithme**

| Option | Verdict | Raison |
| --- | --- | --- |
| ChaCha20-Poly1305 | Retenu | Rapide en logiciel côté hôte, temps constant, crate `chacha20poly1305` (RustCrypto) mature |
| Ascon-AEAD128 | Pour l'embarqué | Standard NIST pour MCU contraints, à retenir si la couche descend sur le nœud |
| AES-GCM | Écarté | Fragile avec un tag tronqué (attaque de Ferguson) |
| Crypto MeshCore | Écarté | Blocs AES indépendants et MAC de 2 octets, forgeable en environ 65 536 essais |

**Nonce implicite (96 bits)** : `src ‖ tag ‖ index ‖ attempt ‖ compteur de session`. Tous les champs sont dans l'en-tête ou connus des deux côtés, rien n'est transmis en plus. L'en-tête entier sert d'AAD.

**Règle absolue** : `attempt` est dans le nonce, pas seulement dans l'AAD. Réémettre avec le même nonce et un AAD différent donnerait deux tags Poly1305 sous la même clé à usage unique, ce qui permet la forge. Un test automatisé doit vérifier qu'aucun nonce n'est jamais réutilisé.

**Tag de 8 octets** : acceptable ici car chaque tentative de forge coûte une émission radio d'au moins 100 ms soumise au duty cycle. Une attaque en ligne sur 2⁶⁴ essais est physiquement impossible.

**Gestion des clés**

1. Secret de départ : le secret X25519 statique que MeshCore dérive déjà entre deux contacts.
2. Ouverture de session : échange de sels aléatoires, clé de session = HKDF-SHA256(secret, sels, contexte). Le compteur de nonce repart à zéro avec une nouvelle clé, jamais avec l'ancienne, même après un reboot.
3. Forward secrecy, recommandée dès qu'un transfert dépasse quelques fragments : X25519 éphémère, 32 octets dans chaque sens, soit un seul paquet.
4. Expiration des clés de session sur durée ou volume, à paramétrer.

## Groupes privés

Chaque membre chiffre avec sa propre sender key, sur le modèle de Signal. Les messages de groupe voyagent en flood dans une enveloppe `GRP_DATA` et sont limités à du texte et à des annonces courtes.

**Transport**

- Enveloppe : `GRP_DATA` = `channel_hash(1) | nos données`. Les relais la floodent sans la déchiffrer.
- Envoi par `CMD_SEND_RAW_PACKET`, réception par `PUSH_CODE_LOG_RX_DATA`. Aucun firmware modifié.
- Effet de bord connu : un nœud stock abonné à un canal de même hash affichera du bruit environ une fois sur 65 536. Choisir un `channel_hash` peu utilisé.
- Limites : 1 à 2 fragments par message, nombre de sauts plafonné. Images et voix ne sont jamais floodées : on floode une annonce, le contenu passe par le pont (voir plus bas).

**Crypto de groupe**

1. **Sender key par membre** : une clé commune unique permettrait à n'importe quel membre d'usurper les autres. Chaque membre distribue sa sender key aux autres par les canaux pairwise X25519.
2. **Nonce** : il inclut l'ID de l'émetteur, pour qu'aucun nonce ne soit partagé entre deux membres.
3. **Rekey** : redistribution des sender keys à chaque départ d'un membre. MLS est trop lourd pour la LoRa.
4. **Non-répudiation (optionnelle)** : signature Ed25519 de 64 octets, sur le datagramme réassemblé uniquement, jamais par fragment.

## Pont Internet et échelle de transports

La LoRa sert de canal de signalisation authentifié : une annonce d'environ 70 octets, un seul paquet, permet de récupérer un contenu lourd sur Internet par iroh ou Tor. L'intégrité du contenu est garantie par le message LoRa.

**Message d'annonce (chiffré, 1 fragment)**

| Champ | Taille | Contenu |
| --- | --- | --- |
| `content_type` | 1 o | Image, voix, fichier, autre |
| `transports` | 1 o | Bitmask : iroh, onion, LoRa |
| `peer_key` | 32 o | Clé publique Ed25519 : NodeId iroh et/ou clé de service onion v3 |
| `blob_hash` | 32 o | Hash BLAKE3 du contenu |
| `meta` | \~4 o | Taille, codec, durée |
| `blob_key` | 32 o, optionnel | Clé de déchiffrement, en mode asynchrone |

**Échelle de transports** (le destinataire prend le premier qui réussit)

1. **iroh direct** : connexion QUIC par clé publique, hole punching NAT, relais en repli. Transfert par `iroh-blobs`, vérifié en BLAKE3 au fil de l'eau.
2. **Tor onion** : l'adresse onion se déduit de la clé publique de 32 octets. Bibliothèque visée : Arti, l'implémentation Rust officielle. La maturité de l'hébergement d'onion services y est à vérifier.
3. **LoRa fragmentée** : envoi progressif en direct, hors réseau.

**Modes**

- **Synchrone** : l'émetteur sert le blob tant qu'il n'a pas été récupéré.
- **Asynchrone** : l'émetteur dépose le blob chiffré sur un stockage quelconque et envoie `blob_hash + blob_key`.
- **Relais uniquement ou Tor uniquement** : pour les utilisateurs qui ne veulent pas exposer leur IP.

**Sécurité** : l'identité du pair et le hash sont fixés par un message LoRa authentifié (canal pairwise ou sender key de groupe). Un relais Internet malveillant ne peut ni se faire passer pour le pair, ni substituer le contenu. Aucune PKI nécessaire.

## Médias : images et voix

Les médias passent par le pont Internet dès qu'il est disponible. Sur LoRa, seuls les petits médias en direct sont autorisés : des miniatures et des messages vocaux, jamais d'appels.

| Média | Sur LoRa | Sur iroh direct | Sur Tor |
| --- | --- | --- | --- |
| Image | Miniature 160×120 AVIF ou WebP, 2 à 4 Ko, 15 à 25 fragments | Pleine résolution | Pleine résolution |
| Message vocal | Codec2 700C, \~875 o pour 10 s, 6 fragments | Opus | Opus |
| Appel temps réel | Non (duty cycle) | Oui, QUIC + Opus | Déconseillé : latence souvent supérieure à 1 s |

**Coût LoRa de référence (SF8, 62,5 kHz)** : environ 1,4 s d'antenne par fragment plein. Une miniature prend environ 30 s d'émission par saut, et chaque relais consomme aussi les siennes.

**Règles**

1. Jamais de média en flood. En groupe, on floode l'annonce et chaque membre récupère le contenu par le pont.
2. Envoi progressif sur LoRa : un aperçu très compressé de 3 ou 4 fragments d'abord, puis le reste seulement si le destinataire le demande.
3. Contrôle de débit intégré : l'émetteur tient un compteur de temps d'antenne glissant et refuse ou diffère un envoi qui dépasserait le duty cycle.
4. Adaptation au lien : si le SNR remonté par le companion le permet, on passe à un SF plus bas.
5. Codec2 donne une voix robotique mais intelligible. À vérifier : quel binding ou portage Rust prend en charge le mode 700C.

## Partage de localisation

Une position tient en 13 octets chiffrés, environ 30 octets sur l'air avec l'en-tête et le tag (≈ 0,3 s en SF8/62,5 kHz). Elle n'est émise que quand l'écart avec la trajectoire extrapolée dépasse un seuil, et chaque destinataire reçoit la précision qui lui est autorisée.

**Format**

| Champ | Taille | Encodage |
| --- | --- | --- |
| `lat` | 3 o | 24 bits, ≈ 1,2 m |
| `lon` | 3 o | 24 bits, ≈ 2,4 m à l'équateur |
| `alt` | 2 o | Mètres, signé |
| `dt` | 2 o | Secondes depuis le timestamp de session |
| `cap + vitesse` | 2 o | Cap sur 9 bits, vitesse sur 7 bits |
| `précision` | 1 o | Rayon d'incertitude, échelle log |

**Fréquence d'envoi**

- Estimation de trajectoire : émetteur et récepteur extrapolent avec le cap et la vitesse. On émet quand l'écart dépasse le seuil (par exemple 50 m). Rien n'est envoyé à l'arrêt.
- Heartbeat lent pour garantir une mise à jour minimale.
- En groupe (flood) : intervalle minimal de plusieurs minutes, ou mode pull (un membre demande la position en direct).
- Pont disponible : suivi temps réel sur iroh ou Tor, la LoRa reste le repli.

**Vie privée**

1. **Précision par destinataire** : arrondi sur une grille fixe (par exemple 1 km pour un groupe élargi). Jamais de bruit aléatoire : la moyenne de plusieurs messages redonnerait la position exacte.
2. **Fraîcheur** : le timestamp est dans le nonce et l'AAD. Une position plus ancienne que la dernière reçue est rejetée, ce qui empêche le rejeu.
3. **Partage limité dans le temps** (1 h, 8 h…) : la clé de partage expire avec.
4. **Jamais dans les adverts MeshCore.**
5. **Limite physique** : le chiffrement protège le contenu, pas l'émission. Un observateur peut localiser l'émetteur par goniométrie ou RSSI, et des balises régulières se reconnaissent. Pour un usage sensible : intervalles irréguliers et mode pull.

**SOS** : un flag qui déclenche une précision maximale, une priorité haute et un droit exceptionnel au flood vers tous les contacts de confiance.

## Mode recherche

Scénario de référence : plus aucun relais, et plusieurs membres du groupe équipés de ThinkNode M5 cherchent un autre membre. Un seul paquet émis par la cible est reçu par tous les chercheurs à portée : une émission donne N mesures, fusionnées dans un filtre particulaire.

**Matériel : ThinkNode M5 (Elecrow)**

| Élément | Présent | Usage |
| --- | --- | --- |
| SX1262, jusqu'à +22 dBm | Oui | Balise, pings, mesures RSSI et SNR |
| GPS multi-constellation | Oui | Position des chercheurs (ancres) et de la cible |
| e-paper 200×200, 2 boutons | Oui | Flèche et distance estimée sans téléphone |
| ESP32-S3, 8 Mo de PSRAM | Oui | Filtre particulaire embarqué (\~1 000 particules) |
| Prise RP-SMA | Oui | Antenne Yagi 868 MHz pour les caps |
| Batterie 1 200 mAh | Oui | Contraint l'intervalle de balise |
| Boussole, IMU | Non | Caps via le téléphone ou techniques sans boussole |

**Fonctionnement**

1. **Les chercheurs forment le réseau** : option répétition activée sur leurs companions. Un M5 posé sur un point haut sert de relais temporaire.
2. **Balise SOS de la cible** : GPS si disponible, batterie, compteur, intervalle adaptatif. Elle répond aussi aux pings des chercheurs.
3. **Mesure partagée (\~16 o, flood dans le groupe)** : position GPS du chercheur, RSSI, SNR, horodatage, cap optionnel.
4. **Fusion** : filtre particulaire (moindres carrés pondérés pour l'initialisation), qui combine distances RSSI, caps et mesures cumulées pendant les déplacements. Affichage sous forme de carte de probabilité qui se resserre.

**Sources de mesure**

| Méthode | Matériel | Précision réaliste |
| --- | --- | --- |
| RSSI → distance | M5 stock | Zone de centaines de mètres à quelques km. ±1 dB ≈ ±8 % de distance, et les obstacles ajoutent 6 à 8 dB |
| Masque du corps | M5 stock | Direction grossière : minimum de signal quand le corps fait écran |
| Gradient en marchant | M5 stock + trace GPS | Indique si l'on se rapproche, sans boussole |
| Yagi + boussole du téléphone | Yagi sur RP-SMA | Cap à ±10 à 20° en terrain dégagé |
| Antennes sectorielles commutées | Station DF dédiée, firmware modifié (`GetRssiInst`) | Cap à ±20 à 30° en un seul paquet |
| Angle d'arrivée par phase | SDR cohérent (ex. KrakenSDR) | Quelques degrés, station fixe |
| Temps de vol | SX1280 (2,4 GHz) | Bonne, mais portée courte |

Ordre de grandeur : à 1 km, un cap à ±10° donne environ 175 m d'écart latéral. Deux caps croisés à angle droit donnent une zone d'environ 200 m de côté.

**Pièges de terrain**

- Saturation sous environ 100 m : la cible baisse sa puissance sur demande, ou le chercheur ajoute un atténuateur.
- SF de la balise : SF12 pour détecter (environ +10 dB de portée par rapport à SF8, mais environ 6 fois plus de temps d'antenne), puis SF bas pour le suivi rapproché.
- Le duty cycle s'applique aussi en recherche.

**Consentement** : localiser un membre suppose son opt-in préalable. La chasse à un émetteur hostile (brouilleur, nœud malveillant) relève d'une autre politique.

## Contraintes radio et réglementaires

C'est le temps d'antenne qui limite le protocole, pas la bande passante. Toute fonction doit tenir dans le duty cycle européen applicable à sa sous-bande, avec un budget calculé côté hôte.

| Charge | Taille sur l'air | Temps d'antenne (SF8, 62,5 kHz) |
| --- | --- | --- |
| Position | \~30 o | \~0,3 s |
| Fragment plein | 255 o | \~1,4 s |
| Message vocal de 10 s | 6 fragments | \~8 s par saut |
| Miniature d'image | 15 à 25 fragments | \~20 à 35 s par saut |
| Fragment plein en SF12, 125 kHz | 255 o | \~9 s |

- Dans la sous-bande à 10 %, un nœud dispose d'environ 360 s d'émission par heure. Les relais dépensent aussi leur budget pour nos paquets.
- L'application tient un compteur glissant par nœud et affiche le budget restant avant les envois coûteux.
- Valeurs à confirmer : la sous-bande et le preset (SF, BW) effectivement utilisés par le réseau MeshCore ciblé.

## Modèle de menace et sécurité

Le protocole protège le contenu et l'authenticité des messages contre un attaquant radio actif. Il ne protège ni contre la localisation physique des émetteurs, ni contre l'analyse du trafic.

| Menace | Contre-mesure | Risque résiduel |
| --- | --- | --- |
| Écoute passive | AEAD par fragment | Métadonnées visibles : hash src/dst, taille, rythme |
| Fragment forgé ou empoisonnement du buffer | Tag vérifié avant d'accepter un fragment, tag de datagramme aléatoire | DoS par saturation du canal |
| Rejeu | Timestamp et compteur dans le nonce, rejet des messages plus anciens | Aucun connu |
| Réutilisation de nonce | `attempt` dans le nonce, nouvelle clé de session après reboot, test automatisé | Bug d'implémentation |
| Usurpation dans un groupe | Sender key par membre, signature optionnelle | Un membre légitime peut encore mentir sur sa propre position |
| MITM sur le pont Internet | Clé du pair et hash fixés par le message LoRa authentifié | L'opérateur du relais voit qui parle à qui (évitable avec Tor) |
| Exposition d'IP | Mode relais uniquement ou Tor uniquement | Aucun dans ces modes |
| Localisation de l'émetteur | Intervalles irréguliers, mode pull | Goniométrie et RSSI restent possibles |
| Spoofing de position | Cohérence avec les mesures RSSI et caps du groupe | Détection seulement, pas de preuve |
| Fausse distance par RSSI (puissance poussée) | Temps de vol (distance bounding) si SX1280 | RSSI seul non fiable face à un attaquant |
| Brouilleur ou nœud malveillant | Chasse par radiogoniométrie (mode recherche) | Pas de prévention |

Cible de fuzzing prioritaire : les parseurs d'en-tête de fragment, d'ACK, d'annonce et du protocole série companion. Tout le reste est chiffré et authentifié avant d'être parsé.

## Questions ouvertes et prochaines étapes

Prochaine étape : les maquettes des apps (SwiftUI pour iOS, Jetpack Compose pour Android), puis un cœur Rust partagé par les deux.

**Questions ouvertes**

- [ ] Sous-bande et preset LoRa (SF, BW) du réseau cible : ils fixent tous les budgets de temps d'antenne.
- [ ] Maturité de l'hébergement d'onion services dans Arti.
- [ ] Binding ou portage Rust de Codec2 qui prend en charge le mode 700C.
- [ ] Valeur de `channel_hash` pour l'enveloppe de groupe.
- [ ] Durée de vie et volume maximum d'une clé de session.
- [ ] Précision RSSI réelle sur le terrain visé (à mesurer avant de concevoir le solveur).

**Plan**

1. Maquettes iOS et Android : conversations, groupes, carte et localisation, mode recherche, budget de temps d'antenne.
2. Cœur Rust (`no_std` pour la fragmentation et la crypto), exposé à Swift et Kotlin via UniFFI. Client du protocole série/BLE companion.
3. Harness de test avec simulation de pertes et fuzzing des parseurs.
4. Outil d'enregistrement terrain : RSSI, SNR, GPS et cap, avec 3 ou 4 nœuds à positions connues.
5. Pont iroh, puis Tor.
6. Firmware « mode recherche » pour la variante `thinknode_m5` : balise SOS, pings, filtre embarqué, affichage e-paper.

**Sources**

- [MeshCore — Packet Format](https://docs.meshcore.io/packet_format/)
- [MeshCore — Payload Format](https://docs.meshcore.io/payloads/)
- [MeshCore Protocol Specification, section 11 Multipart (communautaire)](https://swaits.github.io/meshcore-spec/v0.1.0/11-payload-multipart.html)
- [Code source MeshCore](https://github.com/meshcore-dev/MeshCore), commit `81dbae5`
- [ThinkNode M5 — fiche technique (Passion Radio)](https://www.passion-radio.com/lpwan-iot/ilm13205d1-3175.html)
- [ThinkNode M5 — fiche technique (Ameridroid)](https://ameridroid.com/products/thinknode-m5-meshtastic-radio-1)
