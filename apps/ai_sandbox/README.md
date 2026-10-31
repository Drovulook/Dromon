# ai_sandbox

Bac à sable où un agent Claude Code pilote le moteur (caméra, temps, logs, captures
d'écran) et modifie la config et les assets de cette app, sans toucher au code. Pour
l'instant : le monde de Physis, avec sa propre copie de `config/` et `res/` (les assets
de Physis restent intacts).

## Fichiers
```
ai_sandbox/
├── src/main.rs            scène (code : session principale uniquement)
├── config/                .ron rechargés à chaud            — agent : écriture
├── dev_res/               sources des assets (versionnées)    — agent : écriture
├── res/                   lu par le moteur en jeu (gitignoré) — agent : lecture seule
├── work/                  intermédiaire jetable (gitignoré)   — agent : lecture seule
├── CLAUDE.md              instructions de l'agent
├── .mcp.json              déclare le pont MCP (claude_mcp/dromon_mcp.py)
└── .claude/settings.json  permissions de l'agent
```
Le pont lui-même est à la racine du dépôt, dans `claude_mcp/` : hors de portée de l'agent.

Critère de rangement : supprimer `dev_res/` perd du travail ; supprimer `res/` empêche le
jeu de démarrer (ou force à régénérer) ; supprimer `work/` ne coûte rien. Exemples dans
`work/` : `macro_maps/` (PNG de la carte du monde, réécrits à chaque recalcul).

## Lancer
| Terminal | Commande |
|---|---|
| 1 — moteur | `./launch.py --ai_sandbox` (racine du dépôt) |
| 2 — agent | `cd apps/ai_sandbox && claude` |

L'ordre est libre : le pont se reconnecte au moteur toutes les secondes.

**Premier lancement de l'agent** : accepter la confiance dans le dossier (sinon les
permissions du projet ne s'appliquent pas). Puis `/mcp` doit afficher `dromon` connecté.
Au tout premier démarrage du pont, `uv` installe le SDK `mcp` (quelques secondes, en cache
ensuite).

## Permissions (`.claude/settings.json`)
- Mode `dontAsk` : tout ce qui n'est pas dans `allow` est refusé **sans demander**.
- `allow` : outils du pont, modification de `config/**` et `dev_res/**`. La lecture est
  libre dans ce dossier.
- `deny` (prime sur tout, quel que soit le mode choisi avec Shift+Tab) : le shell, `res/`,
  `src/`, `Cargo.toml`, et les fichiers qui définissent l'agent (sinon il pourrait
  s'accorder des droits). `work/` n'est pas dans `allow`, donc refusé en `dontAsk`.
- Chemins : `/x` = relatif à ce dossier ; `//x` = absolu.
- Hérités : `~/.claude/settings.json` et le `.claude/settings.local.json` **de la racine
  du dépôt** (approbations « ne plus demander » de la session principale). Les `deny`
  d'ici restent prioritaires.
- Limite : les permissions encadrent les outils de Claude, pas les programmes qu'un outil
  lance.

## Dépannage
- `logs_tail` / outils : « moteur injoignable » → moteur pas lancé, ou planté (`[BRIDGE]
  connexion au moteur perdue` dans les logs).
- `/mcp` : `dromon` en échec → `claude mcp list` depuis ce dossier, et tester le pont seul :
  `uv run --script claude_mcp/dromon_mcp.py --app ai_sandbox` (racine du dépôt ; il attend
  du JSON-RPC sur stdin, Ctrl+C pour quitter).
- Commande sans l'agent : `./dromon_ctl.py --app ai_sandbox camera.get`.

## Ajouter un outil
1. Commande côté moteur : `Dromon-engine/src/app/engine/commands.rs` (enum `Command`).
2. Outil côté pont : une fonction `@mcp.tool()` dans `claude_mcp/dromon_mcp.py` ; sa
   docstring est la description lue par l'agent, ses annotations de type le schéma.
3. Le mentionner dans `CLAUDE.md`. Redémarrer la session de l'agent pour qu'il le voie.
