#!/bin/bash
set -e

# Shaders du rendu. Includes depuis dev_res/shaders/ (common/…) ou relatifs (lib/…).
# Les shaders de procgen/ (texgen) sont compilés par le moteur, avec la recette.
SRC=dev_res/shaders/rendering
INC="-I dev_res/shaders"

# Shader des render objects (modèles 3D texturés)
slangc $SRC/object.slang $INC -entry vertMain -stage vertex -o res/shaders/object_vert.spv
slangc $SRC/object.slang $INC -entry fragMain -stage fragment -o res/shaders/object_frag.spv
slangc $SRC/object.slang $INC -entry shadowVertMain -stage vertex -o res/shaders/object_shadow_vert.spv

# Shader du terrain volumétrique (géométrie marching cubes, poids de matériaux)
slangc $SRC/terrain.slang $INC -entry terrainVertMain -stage vertex -o res/shaders/terrain_vert.spv
slangc $SRC/terrain.slang $INC -entry terrainFragMain -stage fragment -o res/shaders/terrain_frag.spv
slangc $SRC/terrain.slang $INC -entry terrainShadowVertMain -stage vertex -o res/shaders/terrain_shadow_vert.spv

slangc $SRC/sky.slang $INC -entry skyVertMain -stage vertex -o res/shaders/sky_vert.spv
slangc $SRC/sky.slang $INC -entry skyFragMain -stage fragment -o res/shaders/sky_frag.spv

# Surface de la mer (grille CDLOD)
slangc $SRC/water.slang $INC -entry waterVertMain -stage vertex -o res/shaders/water_vert.spv
slangc $SRC/water.slang $INC -entry waterFragMain -stage fragment -o res/shaders/water_frag.spv

echo "Shaders compilés."
