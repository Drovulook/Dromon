# ─── Configuration Dromon ─────────────────────────────────────────────────────

# True  : lance engine + CLI, les logs Vulkan sont envoyés au CLI via socket
# False : lance uniquement l'engine, les logs s'affichent dans le terminal
USE_CLI = True

# True : compile en --release (optimisé, pas de validation layers)
# False : compile en debug (validation layers actives)
RELEASE = True

# True : passe --enable-profiling au binaire de l'app. À toi d'activer
#        l'instrumentation correspondante dans la librairie (engine).
ENABLE_PROFILING = True
