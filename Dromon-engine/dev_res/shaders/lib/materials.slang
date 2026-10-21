// Table des matériaux (set 4) : SSBO indexé par ID (0 = air, puis materials.ron dans
// l'ordre, puis le matériau de debug). ⚠ Disposition synchronisée avec `GpuMaterial`
// (material_buffer.rs).
#pragma once

struct Material {
  float4 albedo;    // rgb = albédo linéaire (humide si teinté)
  float4 dryAlbedo; // rgb = albédo en zone sèche (= albedo si non teinté)
  float4 humidity;  // x, y = précipitations du fondu sec → humide
  float4 textureParams; // x = couche du texture array (−1 : aucune), y = 1 / taille d'une répétition
};

[[vk::binding(0, 4)]]
StructuredBuffer<Material> materials;

// Textures du terrain (cf. `TerrainTextures`) : couche = textureParams.x,
// rgb = albédo (décodé sRGB → linéaire), a = hauteur du relief.
[[vk::binding(1, 4)]]
Sampler2DArray terrainTextures;

// Albédo du matériau `id` sous `precipitation` (normalisées, cf. `precipitation()`).
// Même fondu pour tous : un matériau non teinté a `dryAlbedo == albedo`.
float3 materialAlbedo(uint id, float precipitation) {
  Material m = materials[id];
  float wet = smoothstep(m.humidity.x, m.humidity.y, precipitation);
  return lerp(m.dryAlbedo.rgb, m.albedo.rgb, wet);
}

// Exposant des poids triplanaires : plus il est grand, plus le fondu entre deux
// projections est étroit (1 = fondu large et flou ; 4 = usuel).
static const float TRIPLANAR_SHARPNESS = 4.0;

// Poids sous lequel une projection n'est pas lue : sur un sol plat ou une paroi, une
// seule lecture au lieu de 3.
static const float MIN_PROJECTION_WEIGHT = 0.01;

// Poids des projections selon X, Y, Z pour la normale `N` : |N|^k normalisé, somme 1,
// les négligeables mis à 0 (puis renormalisé).
float3 triplanarWeights(float3 N) {
  float3 w = pow(abs(N), TRIPLANAR_SHARPNESS);
  w /= w.x + w.y + w.z;
  w = select(w < MIN_PROJECTION_WEIGHT, 0.0, w);
  return w / (w.x + w.y + w.z);
}

// Position monde et ses dérivées écran, pour lire les textures avec des gradients
// explicites : les lectures ont lieu dans des branches qui varient d'un pixel à
// l'autre, où les dérivées implicites (différences dans le quad 2×2) sont indéfinies.
struct SurfacePoint {
  float3 pos;
  float3 dx; // ddx(pos), calculé hors de toute branche
  float3 dy; // ddy(pos)
};

// Texel triplanaire (rgb = albédo linéaire, a = hauteur) de la couche `layer`, à
// `scale` répétitions par unité monde. Projection selon X → plan YZ, etc. ; Z = haut.
float4 triplanarSample(uint layer, float scale, SurfacePoint s, float3 w) {
  float3 p = s.pos * scale, dx = s.dx * scale, dy = s.dy * scale;
  // Projections de poids nul sautées (cf. `triplanarWeights`).
  float4 c = 0.0;
  if (w.x > 0.0)
    c += w.x * terrainTextures.SampleGrad(float3(p.yz, layer), dx.yz, dy.yz);
  if (w.y > 0.0)
    c += w.y * terrainTextures.SampleGrad(float3(p.xz, layer), dx.xz, dy.xz);
  if (w.z > 0.0)
    c += w.z * terrainTextures.SampleGrad(float3(p.xy, layer), dx.xy, dy.xy);
  return c;
}

// Hauteur prêtée aux matériaux sans texture (milieu de [0, 1]).
static const float FLAT_MATERIAL_HEIGHT = 0.5;

// Surface du matériau `id` au point `s` (rgb = albédo, a = hauteur du relief) : sa
// texture (triplanaire) s'il en a une, sinon sa couleur de table sous `precipitation`.
float4 surfaceSample(uint id, SurfacePoint s, float3 w, float precipitation) {
  float4 params = materials[id].textureParams;
  if (params.x < 0.0)
    return float4(materialAlbedo(id, precipitation), FLAT_MATERIAL_HEIGHT);
  return triplanarSample(uint(params.x), params.y, s, w);
}
