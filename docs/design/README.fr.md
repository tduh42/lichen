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
4. **Tout envoi radio affiche son coût avant d'être lancé** : nombre de fragments et secondes d'antenne. Le budget d'antenne restant est toujours à un geste : la ligne radio de Messages ouvre la carte radio, les Réglages l'affichent, et un bandeau prévient quand il est épuisé.
5. **Typographie** : police système pour l'interface (SF Pro, Roboto). Space Grotesk 700 pour le wordmark et les grands chiffres (budget, cap, distance, empreintes, numéros de sécurité), en chiffres tabulaires.
6. **Des conversations calmes** : un message montre son heure et un mot d'état (« Remis ») ; ses fragments, son temps d'antenne et son signal sont dans appui long › Infos, ou sous chaque message avec Réglages › Afficher les détails radio (désactivé par défaut).
7. **Des écrans radio riches** (2026-11-25) : l'onglet Nœuds, la page d'un nœud, la Carte et les Réglages radio montrent d'un coup d'œil les données de la radio. Chaque nœud entendu directement porte son signal, noté par rapport à ton propre préréglage (`SignalMeter`) ; les autres disent par combien de répéteurs ils passent ; la page d'un nœud dessine la route qui t'atteint, tronçon par tronçon (`RouteRibbon`), son signal sur 24 heures ou 7 jours sur les bandes Bon et Moyen de ton préréglage, et ce qu'il envoie (`ReadingsCard`) ; la Carte trace cette route entre les positions des répéteurs ; les Réglages radio s'ouvrent sur la radio elle-même (nom, modèle, liaison, batterie, temps d'antenne restant, bruit de fond, firmware) et finissent par ce qu'elle a compté depuis son démarrage ; pendant qu'une connexion lit la radio, la ligne radio dit à quelle étape elle en est. Une valeur que rien n'a mesurée n'est jamais affichée : un tronçon non mesuré dit « pas mesuré », une valeur que la radio n'a pas donnée n'a pas de ligne. Ce qu'envoie un nœud ne se demande qu'à la main : ouvrir sa page ne demande rien. Les conversations restent calmes (6).
8. **Accessibilité** : cibles ≥ 44 pt (iOS) / 48 dp (Android), contraste texte ≥ 4,5:1, libellés d'accessibilité sur tous les boutons icône.

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
| `sos` | #EF4444 | #B91C1C | Détresse, action destructrice, signal faible |
| `ok` | #4ADE80 | #15803D | Clé vérifiée, remis, bon signal, entendu ces 2 h |
| `warn` | #FACC15 | #A16207 | Attention, à vérifier, signal moyen |

Chaque couleur sémantique a une variante `_bg` (fond de badge ou de bannière) et `_fg` (texte dessus).

## Typo, espacements, rayons

- iOS : grand titre 34/700, corps 16, secondaire 13–14, légende 12 (le style `.caption` : `.caption2` ne rétrécit pas avec la taille de texte, ce que l’audit d’accessibilité signale).
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
| Conversations | `conversations` | État radio sur une ligne (un geste : budget d'antenne, transports disponibles) ; pendant qu'une connexion lit la radio, l'étape où elle en est : ses réglages, ses contacts (« contacts 34 sur 120 », le nombre annoncé par la radio), ses canaux, puis les messages qui l'attendaient (tant jusqu'ici), sans deviner le temps restant ; liste avec badge de transport et non-lus |
| Discussion | `chat` | Texte, envoi fragmenté avec progression et ACK, photo annoncée (aperçu LoRa + téléchargement iroh/Tor), vocal Codec2, coût estimé dans la zone de saisie |
| Envoi de photo | `media` | Choix du mode : aperçu LoRa, iroh, Tor, différé. Coût par option |
| Nœuds | (dans les apps) | Les nœuds que la radio connaît : « À portée », entendus directement, chacun avec son `SignalMeter`, triés par signal ; « Par des répéteurs », chacun avec ses sauts, ou « chemin inconnu » en dernier ; filtres (Tous, Personnes, Répéteurs, Salons, Entendus en 2 h) ; distance et direction ; dernière écoute |
| Page d'un nœud | (dans les apps) | En-tête avec la grande jauge ou les sauts ; « Comment il t'atteint » (`RouteRibbon`, Tracer à nouveau, Voir sur la carte) ; son signal sur 24 h / 7 jours, un point par mesure de la couleur de sa note, sur les bandes Bon et Moyen de ton propre préréglage (`signal_limits()` du cœur, aucune tant que les réglages de la radio ne sont pas lus) ; « Ce qu'il envoie » (`ReadingsCard`), sauf sur la page de ta propre radio ; puis les cartes salon, administration du répéteur et clé MeshCore |
| Carte et position | `carte` | Membres et précision partagée, partage limité dans le temps, bouton SOS ; Positions ou Localiser en haut ; la couche des nœuds, avec un anneau de la couleur de sa note autour de chaque nœud entendu directement ; la route d'un nœud, chaque tronçon de la couleur de son dernier signal, avec une légende ; tant qu'une route est tracée, ses étapes restent visibles et les autres marqueurs s'estompent, lus « pas sur cette route », jusqu'à ce que sa carte se ferme ; le menu des couches filtre les nœuds avec les puces de l'onglet Nœuds et leurs nombres, le choix de la Carte à part de celui de l'onglet Nœuds |
| Localiser (mode recherche, un mode de la Carte) | `recherche` | Bannière SOS, zone probable, caps des chercheurs, flèche cap + distance + incertitude, mesures, actions Pinger / Ajouter un cap / Baisser sa puissance |
| Groupe | `groupe` | Membres et état de leur clé, rotation des clés après un départ, consentement à être localisé, limite de sauts |
| Réglages | `reglages` | Radio connectée, budget, preset, mode répéteur, iroh / Tor / relais uniquement, précision par défaut ; une tuile sur chaque ligne |
| Réglages radio | (dans les apps) | La radio en haut (nom, modèle, liaison, batterie, temps d'antenne restant, bruit de fond « −118 dBm », firmware) ; une recherche qui va droit à un réglage ou à une valeur ; des sections en cartes ; chaque changement dit sur sa ligne si la radio l'a enregistré ; en dernier, « Depuis son démarrage » : allumée depuis, paquets reçus et envoyés, reçus avec erreurs (et leur part des reçus), temps d'antenne en émission et en réception (et sa part du temps), dernier paquet entendu (dBm · dB, sans couleur de note : il peut venir de n'importe qui), chaque ligne seulement une fois que la radio l'a donnée, relus chaque minute tant que l'écran est ouvert |
| SOS | `sos` | Bouton à maintenir 3 s, contenu envoyé, destinataires, mode d'envoi |
| SOS reçu | `sos-recu` | Alerte plein écran, dernière position, batterie, lancer la recherche |
| États dégradés | `etats` | Bannières : radio déconnectée, budget épuisé, pas de chemin, envoi abandonné, pair injoignable, remis. État vide |
| Écrans M5 | `epaper/sos`, `epaper/recherche`, `epaper/mesure` | Balise SOS active, flèche et distance, prise de cap à la Yagi |

## Navigation

- Premier lancement : Accueil → Appairage → Identité → Ajouter un contact → Conversations.
- Onglets : Messages, Nœuds, Carte, Réglages. Localiser (le mode recherche radio : « Rechercher » ne sert qu'aux messages) est un mode de la Carte, Positions | Localiser en haut, à côté du bouton SOS.
- SOS reçu → la Carte en mode Localiser.
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
| `SignalMeter` | rating (bon, moyen, faible, aucun), snrDb : trois barres en `ok`, deux en `warn` ou une en `sos`, le reste en `line`, et « 7,5 dB » en chiffres tabulaires ; aucun ne montre rien ; petit dans les lignes, grand dans les en-têtes ; lu « Signal bon, 7,5 dB » |
| `HopPill` | « 1 saut », « 2 sauts » |
| `RouteRibbon` | stops (toi, chaque répéteur, le nœud ; un hash partagé montre ses noms joints par « / »), legs (un trait de la couleur de son dernier signal, son SNR au-dessus ; en pointillé `line` et « pas mesuré » quand rien ne l'a mesuré) |
| `ReadingsCard` | lignes : batterie « 3,92 V · environ 69 % », température « 4 °C », chaque autre valeur selon son type, « Humidité · capteur 2 » quand elle ne vient pas du nœud lui-même, chaque ligne lue d'un bloc ; « Envoyé il y a 18 min » et Redemander ; la courbe de sa batterie sur 7 jours dès que deux mesures en ont une ; sans mesure encore, quoi faire et Demander, ou pour un répéteur ou un salon où tu n'es pas connecté, qu'il ne répond qu'à ceux qui sont connectés, au moins en invité, et Se connecter ; « Demande en cours… » ; puis, jusqu'à la demande suivante, ce qu'elle a donné : pas de réponse, dit pour une personne, qui ne répond que si elle l'autorise, ou pour un serveur, peut-être hors de portée ; une réponse sans rien que lichen lit ; la radio ne peut pas le lui demander, il n'est pas dans ses contacts, sans bouton ; une position qu'il envoie est listée, jamais mise sur la carte |

## Notes d'implémentation

- iOS : SwiftUI, `NavigationStack`, `TabView`, listes `.insetGrouped`, `.sheet` pour l'envoi de photo.
- Android : Jetpack Compose, Material 3, `Scaffold` + `NavigationBar`. Désactiver les couleurs dynamiques (Material You) pour garder la marque.
- L'interface observe les états exposés par le cœur Rust (fragmentation, crypto, transports, solveur de recherche) via UniFFI. Pas de logique protocole dans l'UI.
- Le fond de carte des maquettes est schématique : bibliothèque de cartes hors ligne à choisir.
- Écrans M5 : 200×200 en noir et blanc pur, pas de gris (e-paper), deux boutons (appui court / appui long).
- Données affichées dans les maquettes (noms, valeurs radio) : exemples uniquement.
- Icône d'app : `assets/app-icon-1024.svg` (fond encre, chirp orange). Variante : `app-icon-alt-1024.svg`.
