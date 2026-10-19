// Cartes du monde (set 3) : grilles de la carte macro, lisibles par les vertex et
// fragment shaders. ⚠ Bindings synchronisés avec `WorldMaps` et `WORLD_MAP_COUNT`.
#pragma once

#include "bindings.slang"

// Distance signée à la côte (voxels, > 0 terre).
[[vk::binding(0, 3)]]
Sampler2D coastDistanceMap;

// Climat, sur la grille climatique : R = température de l'air (°C), G = précipitations
// (1 = côte humide de référence), B = SST (°C), A = libre.
[[vk::binding(1, 3)]]
Sampler2D climateMap;

// Chunks installés (1 = terrain affiché), adressage torique : chunk c → texel c mod N.
// Lu par `Load` (texel exact, sans filtrage). Réécrit en cours de jeu (`LoadedMask`).
[[vk::binding(2, 3)]]
Sampler2D loadedChunksMap;

// ⚠ Synchronisé avec `CHUNK_SIZE` (terrain_generation/chunk).
static const float CHUNK_SIZE = 64.0;

float2 worldMapUv(float2 xy) {
  return (xy - ubo.worldMaps.x) * ubo.worldMaps.y;
}

float2 climateUv(float2 xy) {
  return (xy - ubo.worldMaps.z) * ubo.worldMaps.w;
}

// Mip 0 explicite : utilisable dans le vertex shader (pas de dérivées pour choisir un
// mip), et les cartes n'en ont qu'un.
float coastDistance(float2 xy) {
  return coastDistanceMap.SampleLevel(worldMapUv(xy), 0).r;
}

float4 climate(float2 xy) {
  return climateMap.SampleLevel(climateUv(xy), 0);
}

// Le terrain du chunk sous `xy` est-il affiché ?
bool chunkLoaded(float2 xy) {
  uint n, unused;
  loadedChunksMap.GetDimensions(n, unused);
  // N puissance de 2 : `& (N − 1)` = modulo positif, y compris pour c < 0 (complément à 2).
  int2 texel = int2(floor(xy / CHUNK_SIZE)) & int(n - 1);
  return loadedChunksMap.Load(int3(texel, 0)).r > 0.5;
}

// Température de l'air à l'altitude **moyenne** de la cellule climatique (°C).
float airTemperature(float2 xy) { return climate(xy).r; }
float precipitation(float2 xy) { return climate(xy).g; }
float seaSurfaceTemperature(float2 xy) { return climate(xy).b; }
