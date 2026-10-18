# Assistant IA de PhotoCraft : plan

> **Créé le :** 2026-10-18 · **Statut :** plan validé, décisions en §10 · **Auteur :** fork warriordelouest/photocraft

## 1. La vision (ton prompt, complété)

Un **assistant graphiste** intégré à PhotoCraft, sous forme de panneau de chat. Tu décris un flyer
(ou tu montres une image de référence, ou tu ouvres un PSD existant) et l'assistant **construit un
vrai document en calques**, éditable à la main comme si tu l'avais monté toi-même.

Ce qu'il doit savoir faire :

1. **Créer un flyer à partir d'une description.** Exemple : « Flyer carré 2000×2000, fond bleu
   nuit avec halo, photo de l'intervenante à droite fondue dans le fond, titre "Atelier Pratique" en
   dégradé blanc→bleu, bandeau rouge "Art Oratoire", carte d'infos violette avec icônes lieu/date/heure
   et un QR code ».
2. **Reproduire un flyer à partir d'une image de référence** : analyser la composition, la palette,
   la typo et la hiérarchie, puis reconstruire le tout en calques.
3. **Modifier un PSD existant** (fait par toi ou par lui) : changer la photo de l'intervenant, la
   date, le thème de couleurs, le titre… C'est le cas des **séries hebdomadaires** (Propulsion, Talk
   des Élites).
4. **Itérer en conversation** : « titre plus gros », « passe le fond en vert », « la photo est trop à
   gauche ». Tu peux aussi corriger à la main entre deux messages, et l'assistant repart de l'état
   réel du document.
5. **Décliner** le même flyer en plusieurs formats (1:1, 4:5, story 9:16) et l'exporter.

**Principe clé :** l'assistant ne « dessine » pas une image plate. Il pilote les **commandes** de
PhotoCraft (calques, texte, formes, styles, masques…), exactement comme les menus. Le résultat est
donc un PSD propre et modifiable. C'est ce qui le distingue d'un générateur d'images.

## 2. Analyse de tes flyers (regard de graphiste)

J'ai décortiqué les 8 flyers fournis. Ils partagent une grammaire très régulière.

### 2.1 Anatomie récurrente

| Zone | Ce qu'on y trouve | Exemples |
|---|---|---|
| **En-tête** | logo + nom de la marque à gauche, **badges** rectangulaires à droite (« +3 500 Membres » rouge, « +50 Pays » violet) | Propulsion (4 flyers) |
| **Titre** | 2-3 niveaux : mot léger + mot **très gras** + mot en **bandeau** (texte sur rectangle plein) ; texte souvent en **dégradé** (blanc→bleu, rose→violet, blanc→rose) | « Atelier / Pratique / ▬Art Oratoire▬ », « Séance de / travail / ▬exclusive▬ » |
| **Personne** | photo **détourée**, placée à droite ou en bas, **fondue** dans le fond par un masque dégradé, **halo / rim light** derrière, couleurs harmonisées avec le thème | Dr Lorie, Dr Claudel, Franck Lienou |
| **Signature** | nom en **police script** (manuscrite) + nom en capitales, lettres espacées (« D o c t e u r ») | « Claudel Noubissie », « Franck Lienou » |
| **Bloc infos** | **carte à coins arrondis** (couleur pleine ou dégradé) + **icônes** lieu / calendrier / horloge + texte ; **QR code** dans un carré blanc arrondi à côté | 5 flyers sur 8 |
| **Offres / tarifs** | grille de **cartes** arrondies en dégradé, **pastille ronde** avec icône, titre, **prix en pilule**, liste à **coches** ; ou pilules « Déclic ▸ 10.000 XAF » | Offres de participation, Pack FLP |
| **Contact** | pilule bordée avec **icône WhatsApp / téléphone** + numéro au format `(+237) 6 72 52 94 78` ; logos Orange Money / MTN | Ndjoka, Propulsion |
| **Produits** | **packshots** avec ombres portées, mise en scène (balançoire, rideau, monument), éléments flottants **flous de mouvement** (feuilles), drapeaux | Ndjoka (3 flyers) |
| **Fond** | dégradé radial/linéaire, **halos lumineux**, lignes ondulées en filigrane, rayons de lumière, bokeh, rideaux dorés, éclats 3D | tous |
| **Crédit** | « Designed by Warrior Design » **vertical** sur un bord, petit logo multicolore | tous (ta signature) |

### 2.2 Les techniques qui reviennent (donc ce que l'IA doit maîtriser)

- **Dégradés partout** : fond, texte (Incrustation de dégradé), cartes, badges, masques de fusion.
- **Hiérarchie typographique** : 3 à 4 graisses, tailles très contrastées, interlignage serré sur les
  titres, approche (tracking) négative sur les gros titres, espacement large sur les sur-titres.
- **Polices** : une sans-serif géométrique (titres et corps) + une **script** (signatures) + parfois
  une display stylisée (« Talk des Élites »).
- **Formes** : rectangles à coins arrondis, pilules, cercles, carrés arrondis (QR), lignes en pointillés.
- **Styles de calque** : incrustation de dégradé et de couleur, ombre portée (cartes, produits),
  lueur externe (halos), contour (pilules bordées).
- **Photo** : détourage, masque dégradé pour fondre le bas du sujet, réglages (Teinte/Saturation,
  Balance des couleurs, Courbes) écrêtés sur la photo pour la marier au thème, lumière de contour.
- **Mise en page** : marges constantes (~12 % de la largeur), alignements à gauche ou centrés, éléments
  liés et groupés par zone, formats **1:1 (2000×2000)** et **4:5 (1605×2000)**.
- **Thèmes de couleurs** : bleu nuit + rouge + violet (Propulsion), lavande clair (offres), bordeaux
  sombre (séance exclusive), vert (Ndjoka), noir et or (Talk des Élites).

### 2.3 Ce que ton prompt n'avait pas mentionné

| Besoin | Pourquoi | Priorité |
|---|---|---|
| **Kits de marque** (logos, palettes, polices, badges, signature « Warrior Design ») | Propulsion, Ndjoka, Talk des Élites ont chacun leur charte : l'IA doit la réutiliser sans que tu la répètes | haute |
| **QR codes** générés (depuis un lien ou un numéro WhatsApp) | présents sur presque tous les flyers | haute |
| **Bibliothèque d'icônes** (lieu, calendrier, horloge, téléphone, coche, couronne, fusée…) | blocs infos et cartes d'offres | haute |
| **Gabarits de séries** : « même flyer, nouvelle date / nouvel intervenant » | ton cas hebdomadaire, le plus fréquent | haute |
| **Bibliothèque d'assets** (photos déjà détourées, PNG, logos, visuels) | tu prépares toi-même les détourages ; l'IA doit retrouver et placer tes fichiers | haute |
| **Détourage automatique** (cheveux, tresses, locks) | utile plus tard ; reporté (service distant) | plus tard |
| **Harmonisation colorimétrique** sujet / fond | une photo rapportée doit « appartenir » au thème | moyenne |
| **Typographie française** : espaces insécables (« 16h – 19h », « 10.000 FCFA »), format des numéros `(+237)`, des montants (FCFA / XAF), des dates (« Samedi 17 Octobre 2026 ») | erreurs fréquentes et visibles | moyenne |
| **Contrôles qualité automatiques** : texte qui déborde, contraste insuffisant, élément hors marges, orthographe | l'IA doit se relire comme un graphiste | moyenne |
| **Déclinaisons de formats** (post carré, portrait 4:5, story 9:16, bannière) | même campagne, plusieurs réseaux | moyenne |
| **Export** prêt à poster (JPG/WebP optimisés, PNG) + PSD source | livraison client | moyenne |
| **Polices manquantes** : savoir lesquelles sont installées et proposer une alternative | une référence utilise souvent une police que tu n'as pas | moyenne |
| **Génération d'éléments visuels** (fonds, rideaux, objets 3D, feuilles) | Ndjoka / Talk des Élites ; Claude ne génère pas d'images | basse, optionnelle |
| **Historique propre** : chaque demande = une étape d'annulation nommée « IA : … » | pouvoir revenir en arrière d'un clic | haute |

## 3. Ce que PhotoCraft sait déjà faire

Bonne nouvelle : le moteur couvre déjà l'essentiel. Chaque action est une **commande**
(`crates/engine`, environ 700) appelable avec des paramètres JSON. L'IA n'aura qu'à les appeler.

| Besoin du flyer | Commandes existantes |
|---|---|
| Document, plans de travail | `file.new`, `layer.new.artboard`, `image.canvasSize` |
| Calques, groupes, liens | `layer.new.layer`, `layer.new.group`, `layer.groupLayers`, `layer.linkLayers`, `layer.moveTo`, `layer.duplicate`, `layer.renameLayer` |
| Formes | `shape.create` (`rect`, `roundedRect` avec rayons par coin, `ellipse`, `polygon`, `star`, `line`, `path`), remplissage dégradé, contour, `layer.combineShapes.*` |
| Texte | `type.create` (texte de point ou de paragraphe, police, taille, couleur, alignement), `type.setStyle`, `type.editText`, `type.fonts`, styles de caractère et de paragraphe, `type.warpText` |
| Styles de calque | `layer.layerStyle.gradientOverlay`, `.colorOverlay`, `.dropShadow`, `.stroke`, `.bevelEmboss`, `.patternOverlay`, `.copyLayerStyle` / `.pasteLayerStyle` |
| Calques de réglage | `layer.newAdjustmentLayer.hueSaturation`, `.colorBalance`, `.curves`, `.levels`, `.gradientMap`… |
| Masques | `layer.layerMask.revealAll` / `hideAll` / `fromTransparency`, `paint.gradient` sur le masque, `layer.createClippingMask`, `layer.vectorMask.*` |
| Images | `file.placeEmbedded`, `file.placeLinked`, `layer.smartObjects.replaceContents` |
| Détourage | `select.subject`, `layer.removeBackground`, `select.selectAndMask` (méthodes classiques, sans réseau de neurones) |
| Alignement | `layer.align.*`, `layer.distribute.*` |
| Transformations | `edit.transform.*`, `edit.freeTransform` |
| PSD | ouverture et enregistrement fidèles (307 fichiers sur 309 du corpus de test) |
| Variables | `variables.*` (graphismes pilotés par données : base pour les séries hebdo) |
| Pilotage par agent | serveur MCP (`crates/automation`) : `doc_inspect` (arbre des calques en JSON), `doc_render_preview` (PNG), `command_list`, `command_run`, `command_batch` |

## 4. Ce qu'il faut construire

1. **Le panneau de chat** dans l'interface (`crates/ui-egui`), dans le style de ta maquette : fond
   sombre avec halo bleu, cartes de suggestions, champ « Décris ton flyer… », bouton micro, pièces
   jointes (images de référence, photos, logos).
2. **Le moteur d'agent** (nouveau crate `photocraft-assistant`, couche L6 comme `automation`) :
   - client de l'API Claude (HTTP + streaming) ;
   - boucle d'outils : Claude demande une action, PhotoCraft l'exécute, renvoie le résultat, etc. ;
   - **boucle visuelle** : après chaque étape, rendu de l'aperçu, renvoyé à Claude pour qu'il voie ce
     qu'il a fait et corrige.
3. **Des commandes de haut niveau** qui manquent pour les flyers :
   - `ai.qrCode` : génère un QR code en calque de forme (crate pur Rust `qrcode`) ;
   - `ai.icon` : place une icône d'une bibliothèque libre (Lucide ou Phosphor, licence MIT, SVG) ;
   - `ai.textBand` : texte sur bandeau (rectangle + texte, groupés et liés) ;
   - `ai.infoCard`, `ai.priceCard`, `ai.badge`, `ai.pill` : composants fréquents en un appel ;
   - `ai.fadeMask` : masque de fusion en dégradé (« fondre le bas de la photo ») ;
   - `ai.applyTheme` : recolore un document selon une palette (styles, formes, réglages).
4. **Kits de marque et gabarits** : dossier local avec logos, palettes, polices, composants, et la
   description des gabarits de séries (quels calques changent chaque semaine).
5. **La bibliothèque d'assets** : un dossier que tu remplis (photos détourées, PNG, logos, visuels),
   indexé par nom et par dossier, que l'IA consulte et place. Les images jointes dans le chat y sont
   aussi accessibles.
6. **Le client multi-fournisseurs** et le sélecteur de modèle (§6).

## 5. Architecture

```text
┌──────────── PhotoCraft (application) ────────────┐
│ Panneau Chat (ui-egui)                           │
│   │ messages, images jointes                     │
│   ▼                                              │
│ photocraft-assistant (thread de fond)            │
│   │ 1. envoie conversation + aperçu → Claude ────┼──► API Claude
│   │ 2. reçoit des appels d'outils     ◄──────────┼─── (vision + outils)
│   │ 3. les transmet au thread UI                 │
│   ▼                                              │
│ Session du moteur (commandes, historique)        │
│   └─ chaque demande = 1 étape « IA : … »         │
└──────────────────────────────────────────────────┘
```

### 5.1 Les outils donnés à Claude

On ne donne **pas** les 700 commandes une par une : ça coûterait cher en contexte et ça le noierait.
On lui donne une petite boîte à outils générique :

| Outil | Rôle |
|---|---|
| `inspect_document` | arbre des calques en JSON (noms, types, positions, textes, styles) : son « état des lieux » |
| `render_preview` | aperçu du document (environ 1024 px) renvoyé comme image : ses « yeux » |
| `find_commands` | recherche dans le catalogue de commandes (identifiant, paramètres, description) |
| `run_commands` | exécute une liste de commandes en lot, regroupée en une seule étape d'historique |
| `list_assets` / `place_asset` | photos jointes, logos, icônes, kits de marque |
| `list_fonts` | polices installées, avec suggestions d'alternatives |
| `ask_user` | pose une question quand il manque une info (date, numéro, photo) |

Les composants de flyer (`ai.infoCard`, etc.) sont des commandes ordinaires : ils passent par
`run_commands` et sont aussi utilisables à la main et par la CLI.

### 5.2 La boucle de travail d'un flyer

1. **Comprendre** : lire la demande et les images jointes, poser les questions manquantes.
2. **Planifier** : produire une spécification de mise en page (format, grille, zones, palette,
   polices, hiérarchie), visible dans le chat.
3. **Construire** par zones : fond → photo → titre → blocs d'infos → contact → crédit.
4. **Vérifier** : rendre l'aperçu, le comparer à la demande ou à la référence, passer les contrôles
   (débordements, contraste, marges, orthographe), corriger.
5. **Rendre la main** : résumé de ce qui a été fait, avec les calques nommés proprement et groupés.

### 5.3 Les règles de fonctionnement

- **Toujours repartir de l'état réel** : avant chaque demande, l'agent relit le document
  (`inspect_document`), donc tes retouches manuelles sont prises en compte.
- **Annulation** : chaque demande devient une étape d'historique « IA : … », annulable d'un Ctrl+Z.
- **Bouton Stop** : on peut interrompre l'agent à tout moment.
- **Clés API** : une par fournisseur, saisies par toi dans les Préférences et stockées dans le
  coffre de Windows (crate `keyring`), jamais en clair dans les réglages.
- **Ne jamais planter** (règle n°1 du projet) : erreurs réseau, réponse invalide ou commande refusée
  donnent un message dans le chat, jamais un crash.
- **Confidentialité** : les aperçus et images jointes partent vers la passerelle choisie, puis vers
  le fournisseur du modèle. À indiquer clairement dans l'interface, puisque PhotoCraft est « hors
  ligne » par défaut.

## 6. Fournisseurs, modèles et coûts

**Décision (2026-10-18) :** l'assistant passe par des **passerelles multi-modèles** (APIYI et
LaoZhang, sur lesquelles tu as déjà des clés et du solde), avec un **sélecteur de modèle dans le
chat**.

### 6.1 Les deux formats d'API à gérer

| Format | Route | Utilisé pour | Remarque |
|---|---|---|---|
| **Anthropic natif** | `POST {base}/v1/messages` | modèles Claude sur APIYI (`https://api.apiyi.com`) | seul format où le **cache de prompt** est facturé correctement chez APIYI : nettement moins cher sur les longues sessions |
| **OpenAI-compatible** | `POST {base}/v1/chat/completions` | DeepSeek, et LaoZhang (documenté en format OpenAI) | format universel, sans cache Anthropic |

Le crate `photocraft-assistant` définit une interface interne unique (messages, images, outils,
streaming) et deux adaptateurs, un par format. La boucle d'agent ne sait pas quel fournisseur répond.

### 6.2 Configuration

- **Fournisseurs** : nom, URL de base, format, clé API (stockée dans le coffre de Windows).
  Préconfigurés : APIYI (natif + compatible) et LaoZhang (compatible). On peut en ajouter.
- **Modèles** : liste éditable. Chaque entrée indique le fournisseur, l'identifiant exact du modèle
  chez ce fournisseur, et ses **capacités** (vision, outils), car la boucle visuelle exige la vision.
- **Sélecteur dans le chat** : Claude Opus 5.5, Claude Sonnet 5.5, Claude Haiku, Claude Sonnet 4.5,
  DeepSeek V4 Flash. Les identifiants exacts et la disponibilité de chaque modèle sont à vérifier
  sur chaque passerelle en phase 0 (APIYI recommande par exemple `claude-opus-4-8` et
  `claude-sonnet-4-6` dans sa documentation).
- **Modèle sans vision** (à vérifier pour DeepSeek V4 Flash) : l'assistant reste utilisable mais
  « travaille à l'aveugle », uniquement via `inspect_document`, sans aperçu ni image de référence.
  Le sélecteur l'indique.

### 6.3 Coûts

Leviers intégrés dès le départ : **cache de prompt** (format natif), aperçus limités à ~1024 px,
réglage d'« effort » par type de tâche, et **compteur de tokens et coût estimé** affiché par
conversation.

**Estimation, à mesurer en phase 0** (prix publics Anthropic ; les passerelles appliquent leurs
propres tarifs) : un flyer complet, c'est environ 30 à 60 actions et 5 à 10 aperçus, soit de
l'ordre de 1 à 3 $ avec Opus 5.5 et environ la moitié avec Sonnet 5.5. Une modification simple de
gabarit (date + photo) devrait coûter quelques centimes.

## 7. Les trois scénarios principaux en détail

### A. « Fais-moi un flyer… »

Description → questions manquantes (format ? photo ? kit de marque ?) → spécification →
construction → aperçu → corrections → livraison. Tu continues ensuite par messages ou à la main.

### B. « Reproduis ce flyer »

L'image de référence est analysée : grille, zones, palette (couleurs échantillonnées), polices
(identifiées approximativement, avec alternatives installées), effets. Puis construction et
**comparaison côte à côte** avec la référence, et itérations jusqu'à ressemblance satisfaisante.
Limites honnêtes : les photos et éléments 3D de la référence ne sont pas « récupérables » proprement,
il faut fournir les tiennes (ou passer par la génération d'images, §10.3). Les polices exactes non
installées sont remplacées par les plus proches.

### C. « Flyer de la semaine : nouvelle date, nouvel intervenant »

Tu ouvres le PSD de la semaine dernière. L'assistant reconnaît la structure (calques nommés : titre,
intervenant, date, heure, lieu…), remplace la photo (détourage, mise à l'échelle, masque de fondu,
harmonisation), met à jour les textes et, si besoin, change le thème de couleurs. Pour aller plus
vite, il peut **enregistrer le gabarit** (quels calques varient) dans le kit de la série : la fois
suivante, il suffit de dire « Propulsion, samedi 24, Dr X, photo jointe ».

## 8. Phases de réalisation

| Phase | Contenu | Résultat visible |
|---|---|---|
| **0. Socle** | crate `photocraft-assistant`, client multi-fournisseurs (formats Anthropic natif et OpenAI-compatible), clés API, sélecteur de modèle, panneau de chat (ta maquette), outils `inspect_document` / `render_preview` / `find_commands` / `run_commands`, étape d'historique « IA », compteur de coût | « Crée un fond dégradé bleu et un titre centré » fonctionne sur chaque modèle du sélecteur |
| **1. Primitives flyer** | QR code, icônes, bandeau texte, cartes d'infos et d'offres, badges, pilules, masque de fondu, bibliothèque d'assets, placement d'images jointes, polices | un flyer type Propulsion construit de bout en bout depuis une description |
| **2. Œil de graphiste** | boucle visuelle d'auto-correction, règles de design dans les instructions (hiérarchie, marges, contraste), contrôles qualité, typographie française | des résultats propres sans retouche dans la plupart des cas |
| **3. Reproduction** | analyse de référence → spécification → construction → comparaison côte à côte | « reproduis ce flyer » sur tes exemples |
| **4. PSD et séries** | reconnaissance de gabarits, remplacement de photo, mise à jour des textes, `ai.applyTheme`, kits de marque, gabarits enregistrés | le flyer hebdo en un message |
| **5. Photo** | harmonisation sujet/fond (réglages écrêtés), lumière de contour, halo ; détourage automatique via service distant plus tard | photos d'intervenants intégrées comme sur tes flyers |
| **6. Formats** | déclinaisons 1:1 / 4:5 / 9:16 avec réagencement intelligent, export en lot | une campagne complète en un clic |
| **7. (option) Génération** | fonds et éléments générés par un service d'images externe | éléments type Ndjoka / Talk des Élites |

Chaque phase suit les règles du dépôt (`AGENTS.md`) : tests, aucune panique, couches respectées,
build wasm intact.

## 9. Risques et limites

- **Le goût graphique** dépend du modèle : très bon en vision, mais un premier jet demandera souvent
  1 ou 2 allers-retours. La phase 2 sert justement à réduire ça.
- **Précision des positions** : Claude estime les positions d'après l'aperçu. On compense avec
  `inspect_document` (coordonnées exactes) et des commandes de haut niveau (alignement, marges).
- **Polices** : sans les fichiers de police, impossible de reproduire une typo à l'identique.
- **Détourage** : les méthodes classiques actuelles de PhotoCraft sont faibles sur cheveux, tresses et
  locks ; d'où des photos détourées fournies par toi pour commencer.
- **Coût et connexion** : l'assistant nécessite Internet et une clé API payante (le reste de
  PhotoCraft reste hors ligne).
- **Passerelles tierces** : disponibilité, identifiants et capacités des modèles varient selon la
  passerelle et peuvent changer ; les données transitent par elles. Les modèles non-Claude (DeepSeek)
  se comportent différemment avec les outils et la vision : chaque modèle du sélecteur sera testé sur
  le même scénario de référence avant d'être proposé.
- **Marques tierces** (WhatsApp, Google Meet, Orange Money, MTN) : leurs logos ne sont pas
  redistribuables dans le dépôt ; ils vont dans ton kit de marque local.

## 10. Décisions

| Sujet | Décision (2026-10-18) |
|---|---|
| Fournisseur d'IA | passerelles **APIYI** et **LaoZhang** (clés et solde existants), sélecteur de modèle dans le chat : Claude Opus 5.5, Claude Sonnet 5.5, Claude Haiku, Claude Sonnet 4.5, DeepSeek V4 Flash |
| Détourage | tu fournis les assets prêts (photos détourées, PNG, logos) ; détourage distant intégré plus tard |
| Génération d'images | pas pour l'instant |
| Où développer | branche `feature/assistant-ia` de ton fork, phase par phase |

Encore à vérifier en phase 0 : identifiants exacts des modèles sur chaque passerelle, support des
outils et de la vision pour chacun, et si LaoZhang propose aussi le format Anthropic natif.

## Historique des révisions

| Date | Changement | Résumé |
|---|---|---|
| 2026-10-18 | décisions | passerelles APIYI / LaoZhang avec sélecteur de modèle, assets fournis par l'utilisateur, pas de génération d'images |
| 2026-10-18 | création | analyse de 8 flyers, inventaire du moteur, architecture, phases, décisions ouvertes |
