// Cartes du monde (set 3) : grilles de la carte macro, lisibles par les vertex et
// fragment shaders. ⚠ Bindings synchronisés avec `WorldMaps` et `WORLD_MAP_COUNT`.
#pragma once

#include "bindings.slang"

// Distance signée à la côte (voxels, > 0 terre).
[[vk::binding(0, 3)]]
Sampler2D coastDistanceMap;

float2 worldMapUv(float2 xy) {
  return (xy - ubo.worldMaps.x) * ubo.worldMaps.y;
}

// Mip 0 explicite : utilisable dans le vertex shader (pas de dérivées pour choisir un
// mip), et les cartes n'en ont qu'un.
float coastDistance(float2 xy) {
  return coastDistanceMap.SampleLevel(worldMapUv(xy), 0).r;
}
