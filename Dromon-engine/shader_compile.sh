#!/bin/bash
set -e

# Shaders du rendu. Includes depuis dev_res/shaders/ (common/…) ou relatifs (lib/…).
# Les shaders de procgen/ (texgen) sont compilés par le moteur, avec la recette.
SRC=dev_res/shaders/rendering
INC="-I dev_res/shaders"

# `DEBUG_NSIGHT=1 ./shader_compile.sh` : infos de debug dans le SPIR-V (lignes, fonctions,
# source embarquée) pour le profil ligne par ligne de Nsight. Code optimisé inchangé.
OPTS="$INC"
if [ "${DEBUG_NSIGHT:-0}" = "1" ]; then
  OPTS="$OPTS -g"
  echo "Infos de debug activées (Nsight)."
fi

# Shader des render objects (modèles 3D texturés)
slangc $SRC/object.slang $OPTS -entry vertMain -stage vertex -o res/shaders/object_vert.spv
slangc $SRC/object.slang $OPTS -entry fragMain -stage fragment -o res/shaders/object_frag.spv
slangc $SRC/object.slang $OPTS -entry shadowVertMain -stage vertex -o res/shaders/object_shadow_vert.spv

# Shader du terrain volumétrique (géométrie marching cubes, poids de matériaux)
slangc $SRC/terrain.slang $OPTS -entry terrainVertMain -stage vertex -o res/shaders/terrain_vert.spv
slangc $SRC/terrain.slang $OPTS -entry terrainFragMain -stage fragment -o res/shaders/terrain_frag.spv
slangc $SRC/terrain.slang $OPTS -entry terrainShadowVertMain -stage vertex -o res/shaders/terrain_shadow_vert.spv

slangc $SRC/sky.slang $OPTS -entry skyVertMain -stage vertex -o res/shaders/sky_vert.spv
slangc $SRC/sky.slang $OPTS -entry skyFragMain -stage fragment -o res/shaders/sky_frag.spv

# Surface de la mer (grille CDLOD)
slangc $SRC/water.slang $OPTS -entry waterVertMain -stage vertex -o res/shaders/water_vert.spv
slangc $SRC/water.slang $OPTS -entry waterFragMain -stage fragment -o res/shaders/water_frag.spv

echo "Shaders compilés."
