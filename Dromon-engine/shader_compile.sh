#!/bin/bash
set -e

# Shader des render objects (modèles 3D texturés)
slangc dev_res/shaders/object.slang -entry vertMain       -stage vertex   -o res/shaders/object_vert.spv
slangc dev_res/shaders/object.slang -entry fragMain       -stage fragment -o res/shaders/object_frag.spv
slangc dev_res/shaders/object.slang -entry shadowVertMain -stage vertex   -o res/shaders/object_shadow_vert.spv

# Shader du terrain volumétrique (géométrie marching cubes, poids de matériaux)
slangc dev_res/shaders/terrain.slang -entry terrainVertMain       -stage vertex   -o res/shaders/terrain_vert.spv
slangc dev_res/shaders/terrain.slang -entry terrainFragMain       -stage fragment -o res/shaders/terrain_frag.spv
slangc dev_res/shaders/terrain.slang -entry terrainShadowVertMain -stage vertex   -o res/shaders/terrain_shadow_vert.spv

echo "Shaders compilés."
