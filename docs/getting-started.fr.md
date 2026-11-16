# Bien démarrer

*[English version](getting-started.md)*

lichen envoie des messages chiffrés de bout en bout de téléphone à téléphone par des radios LoRa, sans réseau mobile ni Internet. Chaque téléphone parle à sa petite radio en Bluetooth ; les radios relaient les messages, par les répéteurs MeshCore d’autres personnes quand elles sont hors de portée l’une de l’autre. Cette page t’emmène de rien jusqu’au premier message. Il te faut un téléphone, une radio, et quelqu’un d’autre équipé pareil.

## 1. Trouver une radio

lichen fonctionne avec une **radio companion MeshCore** non modifiée : n’importe quelle carte LoRa qui fait tourner le firmware companion de MeshCore (version 1.18 ou plus récente). Il a été testé avec une **Heltec V4** et une **ThinkNode M5** ; les autres cartes que liste le flasheur MeshCore devraient marcher de la même façon. Choisis-la pour la bande de ta région (868 MHz en Europe, 915 MHz en Amérique du Nord), avec son antenne : n’allume jamais une radio LoRa sans son antenne.

Une carte avec écran affiche son code d’appairage ; une carte avec batterie continue de marcher dans une poche.

## 2. Flasher le firmware companion MeshCore d’origine

lichen ne te demande jamais de modifier la radio : il utilise le firmware de MeshCore tel quel.

1. Ouvre le flasheur web de MeshCore, <https://flasher.meshcore.io>, dans Chrome ou Edge sur un ordinateur.
2. Choisis ta carte, puis le firmware **Companion (Bluetooth)**, dans sa dernière version.
3. Branche la radio en USB, suis les étapes du flasheur et laisse la radio redémarrer.

Une radio qui a déjà un firmware companion Bluetooth MeshCore, version 1.18 ou plus récente, n’a besoin de rien.

## 3. Installer lichen

- **Android** (12 ou plus récent) : télécharge [`lichen.apk`](https://github.com/tduh42/lichen/releases/latest/download/lichen.apk) depuis la dernière version publiée, ouvre-le et autorise l’installation.
- **iPhone** : lichen n’est pas sur l’App Store. Avec un Mac et un identifiant Apple gratuit, `scripts/install-ios.sh` le compile et l’installe sur ton iPhone ; la section « On your own iPhone » du README donne les étapes.

## 4. Appairer la radio

1. Allume la radio et garde-la près du téléphone.
2. Ouvre lichen : **Connecter ma radio**. Les radios autour apparaissent ; touche **Appairer** sur la tienne.
3. Le téléphone demande un code à six chiffres : tape celui affiché sur l’écran de la radio. Une radio sans écran utilise `123456`, sauf si quelqu’un l’a changé.
4. lichen montre ensuite ton identité : ouvre **Phrase de récupération** et écris les 24 mots sur papier. Ils restaurent ton identité sur un nouveau téléphone ; personne d’autre n’en a besoin, jamais.

Vérifie ensuite **Réglages › Réglages de la radio** : le préréglage doit être celui des gens autour de toi. En Europe, la plupart des communautés MeshCore utilisent **UE/R.-U. (étroit)** (869,618 MHz, 62,5 kHz, SF 8, CR 4/8). Une radio sur d’autres réglages n’entend personne. Dans le doute, demande à ta communauté MeshCore locale.

Pas encore de radio ? **Essayer le réseau de démo** (sur l’écran d’appairage, ou dans les Réglages) fait tourner la vraie app sur un réseau simulé.

## 5. Ajouter un contact, en personne

Avec l’autre personne à côté de toi, les deux radios allumées :

1. Dans **Messages**, touche le bouton QR code (**Ajouter un contact**).
2. L’un de vous montre **Mon code**, l’autre touche **Scanner** et le vise avec l’appareil photo. Puis échangez.

Scanner en personne rend le contact **vérifié** : lichen sait qu’il parle à son téléphone, sans personne au milieu. Tu peux aussi écrire à toute personne que ta radio a entendue, mais sa clé reste « à vérifier » tant que vous n’avez pas comparé vos numéros de sécurité ou scanné vos codes.

## 6. Envoyer le premier message

Ouvre le contact et écris. Le message est chiffré sur ton téléphone, découpé en paquets radio, envoyé par ta radio, puis rassemblé et déchiffré sur le sien. Quand il arrive, il s’affiche **remis**, avec le temps d’antenne qu’il a pris.

Bon à savoir :

- **Temps d’antenne :** dans cette bande, la radio ne peut émettre que 10 % du temps, 360 secondes par heure. Un texte coûte une fraction de seconde, un message vocal quelques secondes ; lichen refuse ce qui ne tient pas et te dit quand ça tiendra.
- **Hors de portée :** un texte attend jusqu’à 24 heures la radio et l’autre personne, et part dès qu’elles sont de retour.
- **Les gens sans lichen :** tu peux quand même écrire à ceux qui utilisent l’app MeshCore officielle et lire le canal Public ; ces discussions sont celles de MeshCore, pas chiffrées par lichen, et lichen les marque comme telles.

Quelque chose n’a pas marché ? **Réglages › À propos › Signaler un problème** fait un rapport sans tes messages, noms ou positions, que tu peux relire avant de l’envoyer.
