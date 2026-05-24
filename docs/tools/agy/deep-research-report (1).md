# Recherche approfondie sur le mode non interactif d’Antigravity CLI

## Ce que le contrat public permet réellement d’affirmer

En l’état des sources publiques que j’ai pu vérifier, il n’existe pas de spécification officielle, exhaustive et stable du « contrat machine » d’`agy` en mode non interactif. Ce qui est publiquement vérifiable today repose sur un mélange de documentation officielle, de pages de migration, et d’issues du dépôt officiel `google-antigravity/antigravity-cli`. La conséquence pratique est importante : une partie du comportement de `agy -p` est documentée, mais une autre partie n’apparaît que par observation empirique dans les tickets du dépôt. Autrement dit, on peut dresser un contrat **à haut niveau de confiance**, mais pas une grammaire complète et normative comparable à une API JSON officielle. citeturn22search6turn22search3turn65search1turn29view4

Le point le plus important pour une intégration automatisée est le suivant : la surface publique actuelle d’`agy` privilégie le terminal/TUI et non un protocole machine explicite. Une issue officielle décrit trois modes actuels — TUI par défaut, `-i/--prompt-interactive`, et `-p/--print` — et explique que le mode `--print` ne fournit ni streaming, ni annulation de session, ni vrai contrat de reprise multi-session comparable à ACP/JSON-RPC. Une autre issue demande explicitement l’ajout d’un mode `--acp`, précisément parce qu’il n’existe pas encore. citeturn65search1

## Commandes et comportements vérifiés pour le mode non interactif

Le mode non interactif publiquement vérifié d’`agy` est `-p` / `--print`. Une issue du dépôt officiel résume la matrice des modes ainsi : le TUI par défaut et `-i/--prompt-interactive` ont sortie en streaming, approbations d’outils, annulation et état conversationnel ; `-p/--print`, lui, rend une **réponse en bloc**, sans streaming, sans annulation interactive, et avec une logique de conversation distincte du TUI. Cette même source précise aussi que `-p` ne se prête pas encore à l’orchestration externe de type ACP. citeturn65search1

En pratique, les éléments suivants sont publiquement observables pour `-p` :

- `agy -p "…"` et `agy --print "…"` sont les formes vérifiées du mode non interactif. citeturn30view2turn29view1
- `--print-timeout <durée>` est utilisé publiquement dans plusieurs reproductions, par exemple `30s`, `45s` ou `90s`. citeturn30view2turn29view1turn20search1
- `--dangerously-skip-permissions` est utilisé en combinaison avec `-p` pour éviter les demandes d’approbation. citeturn30view2turn20search2
- `--sandbox` existe, mais en `-p` il ne constitue **pas** un mode lecture seule : une issue montre qu’en non interactif, des écritures de fichiers peuvent encore réussir, parce que `--sandbox` restreint surtout les outils terminal/shell et non `write_file`. citeturn30view4turn20search2
- `-c` / `--continue` permet de reprendre la conversation la plus récente ; l’interface imprime même des conseils de reprise du type `agy -c`. citeturn72search2
- `--conversation <uuid>` existe pour **reprendre** une conversation existante ; en revanche, je n’ai trouvé aucune preuve publique qu’`agy -p` accepte aujourd’hui un identifiant fourni par le client pour **créer** une nouvelle session headless de manière idempotente. Au contraire, une issue officielle explique que c’est précisément une fonctionnalité manquante. citeturn29view0

À l’inverse, je n’ai pas trouvé de preuve publique vérifiée pour les options suivantes dans `agy -p` : un `--json` officiel, un `--output <path>`, un `--no-tty`, un `--acp`, ou un `--model` documenté pour le mode non interactif. Sur ces points, les issues du dépôt parlent explicitement de **fonctions demandées mais absentes**, pas d’options déjà livrées. citeturn29view4turn65search1

Enfin, il faut bien distinguer le mode non interactif du système de slash-commands interactifs. La documentation officielle sur les fonctionnalités du CLI montre qu’il existe des commandes du type `/resume`, `/rewind`, `/agents`, `/model`, etc., mais ce sont des surfaces du TUI ; rien, dans les sources publiques retrouvées, ne permet de les traiter comme une API headless séparée du mode `-p`. citeturn37search0turn39search0turn65search1

## Session IDs et reprise de conversation

La politique publique observable autour des Session IDs est la suivante : Antigravity CLI sait **reprendre** des conversations, mais ne publie pas encore un contrat headless propre de type « le client fournit l’ID, `agy` le crée, le réémet et le reprend ensuite » comme le faisait Gemini CLI avec `--session-id`. Une issue officielle compare explicitement Gemini CLI et `agy`, et explique qu’Antigravity ne fournit pas encore d’équivalent stable pour les intégrateurs qui veulent piloter N conversations en parallèle. citeturn29view0turn33search1

Ce qui est vérifié aujourd’hui est plus étroit :

- `--conversation <id>` sert à reprendre une conversation existante. Une issue précise même que, **dans l’état actuel**, si l’ID n’existe pas encore, le comportement attendu est plutôt l’erreur que la création implicite — raison pour laquelle la feature a été demandée. citeturn29view0
- `-c` / `--continue` reprend la conversation la plus récente, ce qui est pratique pour un humain mais insuffisant pour un orchestrateur multi-instance. citeturn29view0turn72search2
- `agy` imprime des conseils de reprise en sortie interactive, par exemple `Resume with: agy --conversation=<uuid>` ou `agy -c`, ce qui confirme l’existence d’un identifiant interne de conversation. citeturn72search2
- En revanche, aucune source publique vérifiée ne montre qu’`agy --print` **retourne** ce Conversation ID dans sa sortie standard, ni sous forme de JSON, ni sous forme de métadonnée stable. Une issue officielle demande justement l’ajout d’une telle fonctionnalité. citeturn29view0

Pour une intégration scriptée, la conclusion est nette : `agy` possède bien un concept de conversation/séance, mais **pas encore un contrat public fiable de Session ID pour le mode headless**. citeturn29view0turn65search1

## Fichiers de configuration, emplacements et formats

Le premier fichier de configuration publiquement documenté pour le CLI est le fichier **global** `~/.gemini/antigravity-cli/settings.json`. La documentation officielle « Using AGY CLI » précise qu’il s’agit d’un fichier JSON en clair, et que ses réglages sont aussi consultables/modifiables via `/config` ou `/settings`. La documentation indique également que les overrides passés par ligne de commande sont visibles comme des surcharges dans l’UI de configuration. citeturn22search6turn25search0

Pour la configuration MCP, la documentation officielle indique qu’Antigravity utilise un fichier distinct `mcp_config.json`, séparé de `settings.json`. La page MCP cite `~/.gemini/antigravity/mcp_config.json`, tandis que la page de migration précise bien qu’Antigravity et Antigravity CLI stockent les MCP servers dans un `mcp_config.json` séparé, contrairement à Gemini CLI. Cependant, côté CLI v1.0.0, une issue officielle montre que les chemins réellement consommés par `agy` sont le chemin historique `~/.gemini/antigravity-cli/mcp_config.json`, puis le chemin migré `~/.gemini/config/mcp_config.json`. Autrement dit : **la doc et le runtime ne sont pas parfaitement alignés publiquement**. citeturn22search1turn22search3turn29view2

Dans un dépôt / workspace, le seul fichier de configuration JSON local vérifié publiquement est `<repo>/.antigravitycli/mcp_config.json`. `agy` le découvre bien au démarrage et le journalise comme « project-local config ». Mais une issue officielle montre aussi que, dans v1.0.0, son champ `mcpServers` est **ignoré** au runtime : le CLI détecte le fichier local, mais ne lance en pratique que les serveurs MCP du fichier HOME-level. citeturn29view2

Le format publiquement vérifié de ce `mcp_config.json` est du JSON avec une clé racine `mcpServers`, elle-même mappée par nom de serveur. Pour les serveurs locaux/stdio, les champs observés sont `command`, `args`, `env` et `disabled`. Pour les serveurs distants, les champs visibles publiquement sont `serverURL` ou `serverUrl` selon la source, ainsi que `headers`, `trust`, `authProviderType` et `oauth`. La documentation officielle précise notamment que `authProviderType` supporte `"google_credentials"` pour ADC, et qu’un bloc `oauth` contient des identifiants de client OAuth. Une issue ajoute que le runtime d’Antigravity rejette encore le vieux champ Gemini CLI `url`, avec le message `serverURL or command must be specified`. citeturn29view2turn52search0turn53search0turn54search0turn55search0turn56search0turn34search2

Concrètement, les formes suivantes sont **vérifiées publiquement** :

```json
{
  "mcpServers": {
    "project_local_unique": {
      "command": "/usr/bin/env",
      "args": ["python3", "/tmp/server.py"],
      "env": {
        "PROJECT_LOCAL_PROBE": "yes"
      },
      "disabled": false
    }
  }
}
```

```json
{
  "mcpServers": {
    "my-server": {
      "serverURL": "https://example.com/mcp/",
      "trust": true
    }
  }
}
```

Ces deux formes proviennent d’exemples publics du dépôt officiel et des snippets officiels de la doc MCP. citeturn29view2turn34search2turn52search0turn53search0

Là où la documentation publique devient incomplète, c’est sur le **schéma complet** de `settings.json`. Ce que j’ai pu vérifier sans extrapoler est le suivant :

- la sélection du modèle courant est bien stockée dans `settings.json`, car des issues reproduisent `agy -p` avec un modèle « set in `~/.gemini/antigravity-cli/settings.json` ». citeturn29view4turn72search1
- un niveau de verbosité y est stocké ; au moins la valeur `low` est publiquement observée. citeturn29view4turn72search1
- un allow-list de permissions existe sous la forme `permissions.allow`, et sa syntaxe inclut des expressions comme `mcp(my-server/*)`. citeturn72search0

En revanche, je n’ai pas retrouvé de schéma public complet, clé par clé, de `~/.gemini/antigravity-cli/settings.json`. Je peux donc documenter les champs **observés**, mais pas fournir honnêtement une liste exhaustive et normative de toutes les variables du fichier global. citeturn22search6turn72search0turn72search1

## Skills, MCP, agents, plugins et autres personnalisations

Pour les **skills**, la documentation officielle est beaucoup plus claire. Antigravity supporte des skills workspace-locales dans `<workspace-root>/.agents/skills/<skill-folder>/`, avec rétrocompatibilité pour `.agent/skills`, et des skills globales dans `~/.gemini/antigravity/skills/<skill-folder>/`. Chaque skill est un dossier contenant un `SKILL.md`; ce fichier commence par un frontmatter YAML où `name` est optionnel et `description` est obligatoire. La doc officielle ajoute qu’au démarrage d’une conversation, l’agent voit d’abord la liste des skills et de leurs descriptions, puis lit le `SKILL.md` complet seulement si la skill semble pertinente. citeturn57search1turn58search0turn15view0turn15view4

Pour les **workflows** et les **rules**, la doc publique montre qu’il s’agit essentiellement de fichiers Markdown. Les rules peuvent être globales ou workspace-specific ; la doc signale que les workspace rules vivent dans `.agents/rules` à la racine du workspace ou du git root. Les workflows sont eux aussi des fichiers Markdown, invoqués comme slash-commands `/workflow-name`, et chaque fichier contient un titre, une description et une séquence d’étapes. Un snapshot de la doc indique une limite de `12,000 characters` par workflow. citeturn66search1turn68search15turn14view3

Pour les **hooks**, la documentation officielle précise qu’ils sont configurés dans un fichier `hooks.json` situé dans le répertoire de customizations — par exemple `.agents/` dans le workspace ou `~/.gemini/config/` pour le global. La même doc indique aussi le contrat d’E/S : les hooks reçoivent des données JSON sur `stdin` et doivent renvoyer du JSON sur `stdout`. C’est un des rares points où un format machine est explicitement documenté. citeturn66search0turn67search0

Pour les **plugins**, la doc officielle dit qu’un plugin est un bundle namespacé qui regroupe skills, rules, MCP servers et hooks ; un snippet de la page des fonctionnalités du CLI ajoute aussi les agents à cette liste. Les plugins locaux se placent dans `.agents/plugins/` ou `_agents/plugins/` à la racine du workspace ; les plugins globaux vont dans `~/.gemini/config/plugins/`. La doc plugins ajoute que les MCP servers d’un plugin se déclarent via un `mcp_config.json` placé à la racine du plugin. citeturn46search0turn49search0turn50search0turn36search0

Pour les **agents / subagents**, la documentation publique explique que le parent décide quels outils et permissions donner aux subagents, notamment l’usage de MCP et la possibilité d’écrire des fichiers. La page sur les subagents précise aussi l’héritage de sécurité : un subagent hérite des préfixes de commandes terminal autorisés et des scopes de lecture/écriture de fichiers de son parent, et ne peut pas obtenir davantage d’accès que lui. À un niveau plus produit, la documentation projet précise aussi que chaque Project maintient ses propres settings et politiques de sécurité que les agents respectent. citeturn38search0turn68search1turn69search0turn68search3turn68search4

Enfin, il faut distinguer ces mécanismes de personnalisation des **context files** du repo. La doc de migration indique explicitement qu’Antigravity CLI lit les mêmes fichiers de contexte que Gemini CLI, à savoir `GEMINI.md` et `AGENTS.md` dans le workspace actif. Ce sont des fichiers de contexte/prompt persistants, pas le même objet que `settings.json` ou `mcp_config.json`. citeturn68search9

## Modèles actifs et manière de les sélectionner

La liste des modèles est le point où les sources publiques sont les plus mouvantes. Les snippets officiels les plus récents de la page Models font apparaître, pour les **reasoning models**, la liste suivante : `Gemini 3.5 Flash`, `Gemini 3.1 Pro (high)`, `Gemini 3.1 Pro (low)`, `Gemini 3 Flash`, `Claude Sonnet 4.6 (thinking)`, `Claude Opus 4.6 (thinking)` et `GPT-OSS-120b`. Une bannière de session observée dans les issues du dépôt montre par ailleurs un modèle courant `Gemini 3.5 Flash (High)`, ce qui suggère des variantes de budget/quality au moins pour certains modèles. citeturn62search0turn63search0turn72search2

Je signale toutefois une limite importante : un snapshot extrait plus ancien de la documentation listait encore `Gemini 3 Pro (high/low)`, `Gemini 3 Flash`, `Claude Sonnet 4.5`, `Claude Opus 4.5` et `GPT-OSS`, tandis qu’un autre snippet officiel plus récent parle désormais de `GPT-OSS-120b` et même d’un modèle d’image `Nano Banana 2` au lieu de `Nano Banana Pro`. Cela montre que le catalogue public évolue vite et que les snapshots/mirrors ne sont pas parfaitement synchronisés. La liste de paragraphe précédent est donc la **meilleure lecture du catalogue actuel**, pas une vérité intemporelle. citeturn16view2turn59search0turn60search0turn62search0

Sur la manière de **sélectionner** le modèle, les sources publiques permettent d’affirmer ceci :

- en usage interactif, le modèle se choisit dans le sélecteur sous la zone de prompt ; le choix est « sticky » à l’intérieur d’une conversation. citeturn16view2
- le CLI expose aussi un picker `/model`, confirmé indirectement par une issue qui cite les slash-command pickers `/model`, `/resume` et `/permissions`. citeturn65search3
- le modèle actif du CLI est stocké dans `~/.gemini/antigravity-cli/settings.json`; une issue de reproduction en `-p` le dit explicitement. citeturn29view4turn72search1

En revanche, **je n’ai pas trouvé de preuve publique vérifiée d’un drapeau `--model` pour `agy -p`**. Les reproductions headless disponibles utilisent le modèle déjà enregistré dans `settings.json`. Si vous devez intégrer `agy` aujourd’hui, la position la plus prudente est donc : le mode non interactif reprend le modèle configuré globalement, et un override `--model` n’est pas publiquement démontré dans les sources retrouvées. citeturn29view4turn72search1

## Erreurs renvoyées, format réel et cas du quota épuisé

Je n’ai trouvé **aucune** source publique qui publie une taxonomie officielle, exhaustive et structurée des erreurs de `agy -p`. Les erreurs observables sont aujourd’hui **humaines et textuelles**, parfois sur la sortie terminal, parfois dans le log, parfois dans le panneau MCP, mais pas dans un schéma JSON normé. Une issue sur le headless le dit d’ailleurs sans détour : `agy -p` devrait idéalement offrir `--json` ou `--output`, mais ce n’est pas fourni aujourd’hui. citeturn29view4turn30view2

Les formes d’erreurs ou d’échecs **publiquement observées** incluent notamment :

- un échec silencieux en mode non interactif sur stdout non-TTY : **code de sortie 0**, **0 octet sur stdout**, **0 octet sur stderr** ; autrement dit, pas de charge utile structurée du tout. citeturn30view2
- `stdin is not a tty (exit 1)` dans un chemin de reproduction `winpty agy …`. citeturn35search1
- `model output error: invalid tool call error (invalid_args) tool <tool> is not enabled for server <server>`. citeturn30view0
- `server <name> is not allowed in this context`. citeturn30view1
- `✗ my-server — error: calling "initialize": sending "initialize": Unauthorized`. citeturn34search2
- `serverURL or command must be specified` quand on fournit l’ancien schéma Gemini-style au lieu du schéma attendu par Antigravity. citeturn34search2
- `Failed to reload MCP config: loading already in progress`. citeturn72search2
- des erreurs d’auth comme `You are not logged into Antigravity` dans les logs. citeturn33search3turn33search7
- côté quota/backend, des utilisateurs rapportent aussi des `429 Resource Exhausted` et des lockouts temporaires. citeturn32view2turn34search1

Sur la question très précise « **que renvoie `agy` en mode non interactif si le quota est épuisé ?** », la meilleure réponse honnête est la suivante : je n’ai pas trouvé de preuve publique d’un **payload structuré** spécifique à `-p`. Le seul libellé explicite que j’ai pu vérifier publiquement est un message CLI humain : **« ⚠ You have exhausted your capacity on this model. Your quota will reset after 2h24m55s. »**. Une autre issue précise par ailleurs qu’il n’existe pas, dans le CLI, de mécanisme apparent pour activer les AI credits lorsque ce quota est épuisé. citeturn32view0turn32view1

Pour une intégration automatisée, cela implique une conclusion peu confortable mais importante : **le cas “quota épuisé” n’a pas de format machine public vérifié**. Et comme `agy -p` peut déjà retourner un succès vide en capture non-TTY, un appelant ne peut pas compter aujourd’hui sur un code/objet d’erreur bien structuré pour distinguer « quota épuisé », « sortie perdue » et « réponse réellement vide ». citeturn30view2turn29view4

## Questions ouvertes et limites documentaires

Plusieurs points restent **ouverts** parce qu’ils ne sont pas publiquement documentés de façon suffisamment stable :

- il n’y a pas de schéma public complet et vérifiable de `~/.gemini/antigravity-cli/settings.json`; seules quelques clés/comportements sont observables publiquement. citeturn22search6turn72search0turn72search1
- la politique exacte des Session IDs en headless n’est pas stabilisée publiquement : `--conversation` reprend, `-c` continue, mais la création idempotente d’une session headless avec ID client et sa réémission en sortie ne sont pas documentées comme supportées. citeturn29view0turn65search1
- les chemins et schémas MCP publics ne sont pas totalement cohérents entre documentation, migration et runtime observé du CLI. citeturn22search1turn22search3turn29view2turn34search2
- le catalogue des modèles est visiblement en transition rapide : les snippets officiels récents, les snapshots extraits et les issues d’intégration ne montrent pas tous exactement la même liste. citeturn16view2turn60search0turn62search0

La synthèse la plus fiable est donc celle-ci : **`agy` a déjà un mode non interactif utile (`-p`), mais pas encore un contrat public, exhaustif et robuste pour l’automatisation lourde**. Si votre objectif est une intégration machine stricte, il faut aujourd’hui traiter le comportement de `-p` comme une interface encore instable, surtout sur les questions de sessions, de sortie non-TTY, de schémas d’erreurs et de quota. citeturn65search1turn29view4turn29view0