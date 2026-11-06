# lichen — passation design

Ce dossier contient tout le design de l'app **lichen** (messagerie chiffrée hors réseau sur MeshCore/LoRa) pour l'implémentation iOS (SwiftUI) et Android (Jetpack Compose). La spec protocole est fournie à part.

Les maquettes dans `mockups/` font foi pour la mise en page. Ce fichier donne les règles, les tokens et la liste des écrans.

## Contenu

| Chemin | Contenu |
| --- | --- |
| `mockups/index.html` | Galerie de tous les écrans, à ouvrir dans un navigateur |
| `mockups/ios/*.html` | 14 écrans iOS, 390×844, liens entre écrans actifs |
| `mockups/android/*.html` | 14 écrans Android, mêmes écrans en Material 3 |
| `mockups/epaper/*.html` | 3 écrans e-paper du ThinkNode M5, 200×200, 1 bit |
| `mockups/logo.html` | Planche du logo (icône, lockups, palette, règles) |
| `tokens.json` | Couleurs sombre et clair, typo, espacements, rayons, tailles |
| `assets/` | Logo « chirp » en SVG, icône d'app 1024 (principale et alternative) |

## Principes

1. **Thème sombre par défaut.** Le thème clair est prévu (colonne `light` de `tokens.json`) et accessible en option.
2. **L'orange signifie LoRa et marque, rien d'autre.** Les actions principales sont blanches en sombre (`act_bg`), encre en clair. Ne jamais utiliser l'orange pour un bouton générique.
3. **Le transport est toujours écrit**, jamais porté par la seule couleur : badge « LoRa · 2 sauts », « iroh », « Tor ».
4. **Tout envoi radio affiche son coût avant d'être lancé** : nombre de fragments et secondes d'antenne. Le budget d'antenne restant est visible en permanence (conversations, réglages).
5. **Typographie** : police système pour l'interface (SF Pro, Roboto). Space Grotesk 700 pour le wordmark et les grands chiffres (budget, cap, distance, empreintes, numéros de sécurité), en chiffres tabulaires.
6. **Accessibilité** : cibles ≥ 44 pt (iOS) / 48 dp (Android), contraste texte ≥ 4,5:1, libellés d'accessibilité sur tous les boutons icône.

## Couleurs (résumé, détail dans `tokens.json`)

| Token | Sombre | Clair | Usage |
| --- | --- | --- | --- |
| `bg` | #0E0E0D | #F7F7F5 | Fond d'écran |
| `s1` | #1A1A18 | #FFFFFF | Cartes, listes, barres |
| `s2` | #262623 | #EFEFEB | Bulles reçues, champs, segments |
| `line` | #34342F | #E2E2DC | Séparateurs |
| `tx` / `tx2` | #F4F4F0 / #ABABA3 | #141413 / #55554F | Texte / secondaire |
| `act_bg` / `act_fg` | #F4F4F0 / #141413 | #141413 / #FFFFFF | Bouton principal, bulle envoyée |
| `lora` | #FB923C | #C2410C | Transport LoRa et marque |
| `iroh` | #60A5FA | #1D4ED8 | Transport iroh |
| `tor` | #A78BFA | #6D28D9 | Transport Tor |
| `sos` | #EF4444 | #B91C1C | Détresse, action destructrice |
| `ok` | #4ADE80 | #15803D | Clé vérifiée, remis |
| `warn` | #FACC15 | #A16207 | Attention, à vérifier |

Chaque couleur sémantique a une variante `_bg` (fond de badge ou de bannière) et `_fg` (texte dessus).

## Typo, espacements, rayons

- iOS : grand titre 34/700, corps 16, secondaire 13–14, légende 11–12.
- Android : titre de barre 22/400, mêmes tailles pour le reste.
- Espacements : 4, 8, 12, 16, 24. Marges d'écran 16.
- Rayons : cartes 14 (iOS) / 16 (Android). Boutons 14 et 50 de haut (iOS) / pilule et 48 de haut (Android). Badges 8.

## Écrans

| Écran | Fichiers | Contenu et états |
| --- | --- | --- |
| Accueil | `accueil` | Logo, promesse en 3 points, « Connecter ma radio », « Plus tard » |
| Appairage | `appairage` | Scan BLE, liste des radios (nom, RSSI, version companion), saisie du code à 6 chiffres affiché sur la radio |
| Identité et clés | `identite` | Pseudo, empreinte de clé (6 groupes de 4 hex), alerte sauvegarde, phrase 24 mots, fichier chiffré |
| Ajouter un contact | `contact` | Onglets Mon code / Scanner, QR, numéro de sécurité (6 groupes de 5 chiffres), « correspondent » / « différents » |
| Conversations | `conversations` | État radio, budget d'antenne, transports disponibles, liste avec badge de transport et non-lus |
| Discussion | `chat` | Texte, envoi fragmenté avec progression et ACK, photo annoncée (aperçu LoRa + téléchargement iroh/Tor), vocal Codec2, coût estimé dans la zone de saisie |
| Envoi de photo | `media` | Choix du mode : aperçu LoRa, iroh, Tor, différé. Coût par option |
| Carte et position | `carte` | Membres et précision partagée, partage limité dans le temps, bouton SOS |
| Mode recherche | `recherche` | Bannière SOS, zone probable, caps des chercheurs, flèche cap + distance + incertitude, mesures, actions Pinger / Ajouter un cap / Baisser sa puissance |
| Groupe | `groupe` | Membres et état de leur clé, rotation des clés après un départ, consentement à être localisé, limite de sauts |
| Réglages | `reglages` | Radio connectée, budget, preset, mode répéteur, iroh / Tor / relais uniquement, précision par défaut |
| SOS | `sos` | Bouton à maintenir 3 s, contenu envoyé, destinataires, mode d'envoi |
| SOS reçu | `sos-recu` | Alerte plein écran, dernière position, batterie, lancer la recherche |
| États dégradés | `etats` | Bannières : radio déconnectée, budget épuisé, pas de chemin, envoi abandonné, pair injoignable, remis. État vide |
| Écrans M5 | `epaper/sos`, `epaper/recherche`, `epaper/mesure` | Balise SOS active, flèche et distance, prise de cap à la Yagi |

## Navigation

- Premier lancement : Accueil → Appairage → Identité → Ajouter un contact → Conversations.
- Onglets : Messages, Carte, Recherche, Réglages.
- SOS depuis la Carte. SOS reçu → Mode recherche.
- Discussion → Envoi de photo en feuille modale (`.sheet` sur iOS, `ModalBottomSheet` sur Android).

## Composants partagés

| Composant | Paramètres |
| --- | --- |
| `TransportBadge` | kind (lora, iroh, tor), label |
| `AirtimeMeter` | remainingS, budgetS |
| `FragmentProgress` | sent, total, retries, etaS |
| `Banner` | kind (sos, warn, lora, iroh, ok), title, body, action optionnelle |
| `MediaAnnouncement` | title, size, transports[], hashVerified |
| `VoiceNote` | durationS, codec, waveform[] |
| `BearingCard` | bearingDeg, distanceM, uncertaintyM |
| `MeasurementRow` | who, rssi, snr, bearing optionnel, age |
| `PinEntry` | 6 chiffres |
| `SafetyNumber` | 6 groupes de 5 chiffres |
| `KeyFingerprint` | 6 groupes de 4 hex |
| `SosHoldButton` | holdMs = 3000, progression circulaire |

## Notes d'implémentation

- iOS : SwiftUI, `NavigationStack`, `TabView`, listes `.insetGrouped`, `.sheet` pour l'envoi de photo.
- Android : Jetpack Compose, Material 3, `Scaffold` + `NavigationBar`. Désactiver les couleurs dynamiques (Material You) pour garder la marque.
- L'interface observe les états exposés par le cœur Rust (fragmentation, crypto, transports, solveur de recherche) via UniFFI. Pas de logique protocole dans l'UI.
- Le fond de carte des maquettes est schématique : bibliothèque de cartes hors ligne à choisir.
- Écrans M5 : 200×200 en noir et blanc pur, pas de gris (e-paper), deux boutons (appui court / appui long).
- Données affichées dans les maquettes (noms, valeurs radio) : exemples uniquement.
- Icône d'app : `assets/app-icon-1024.svg` (fond encre, chirp orange). Variante : `app-icon-alt-1024.svg`.
