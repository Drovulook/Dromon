// Table des matériaux (set 4) : SSBO indexé par ID (0 = air, puis materials.ron dans
// l'ordre, puis le matériau de debug). ⚠ Disposition synchronisée avec `GpuMaterial`
// (material_buffer.rs).
#pragma once

struct Material {
  float4 albedo;    // rgb = albédo linéaire (humide si teinté)
  float4 dryAlbedo; // rgb = albédo en zone sèche (= albedo si non teinté)
  float4 humidity;  // x, y = précipitations du fondu sec → humide
  float4 textureParams; // x = couche du texture array (−1 : aucune), y = 1 / taille d'une répétition
  float4 textureInvMean; // rgb = 1 / albédo moyen de la texture (cf. `surfaceSample`)
};

[[vk::binding(0, 4)]]
StructuredBuffer<Material> materials;

// Textures du terrain (cf. `TerrainTextures`) : couche = textureParams.x,
// rgb = albédo (décodé sRGB → linéaire), a = hauteur du relief.
[[vk::binding(1, 4)]]
Sampler2DArray terrainTextures;

// Mêmes couches, linéaire : rg = normale x, y (espace tangent, NormalGL ; z à
// reconstruire), b = AO.
[[vk::binding(2, 4)]]
Sampler2DArray terrainNormals;

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

// Facteurs du mélange par hauteur (somme 1) de 4 surfaces au plus : score = hauteur
// + poids ; seules celles à moins de `depth` du meilleur score contribuent → le relief
// le plus haut l'emporte au pixel (frontière irrégulière, pas de surimpression).
// Poids nul = entrée absente. → 0 : frontière découpée ; grand : moyenne pondérée.
float4 heightBlendFactors(float4 heights, float4 weights, float depth) {
  bool4 present = weights > 0.0;
  float4 score = select(present, heights + weights, 0.0);
  float best = max(max(score.x, score.y), max(score.z, score.w));
  float4 b = select(present, max(score - best + depth, 0.0), 0.0);
  float total = b.x + b.y + b.z + b.w;
  return total > 0.0 ? b / total : 0.0;
}

// Profondeur du mélange par hauteur entre projections (cf. `heightBlendFactors`) :
// évite la surimpression de deux projections sur les pentes ~45°.
static const float PROJECTION_BLEND_DEPTH = 0.2;

// Texel triplanaire (rgb = albédo linéaire, a = hauteur) de la couche `layer`, à
// `scale` répétitions par unité monde. Projection selon X → plan YZ, etc. ; Z = haut.
float4 triplanarSample(uint layer, float scale, SurfacePoint s, float3 w) {
  float3 p = s.pos * scale, dx = s.dx * scale, dy = s.dy * scale;
  // Projections de poids nul sautées (cf. `triplanarWeights`).
  float4 tx = 0.0, ty = 0.0, tz = 0.0;
  if (w.x > 0.0)
    tx = terrainTextures.SampleGrad(float3(p.yz, layer), dx.yz, dy.yz);
  if (w.y > 0.0)
    ty = terrainTextures.SampleGrad(float3(p.xz, layer), dx.xz, dy.xz);
  if (w.z > 0.0)
    tz = terrainTextures.SampleGrad(float3(p.xy, layer), dx.xy, dy.xy);
  float4 f = heightBlendFactors(float4(tx.a, ty.a, tz.a, 0.0), float4(w, 0.0),
                                PROJECTION_BLEND_DEPTH);
  return f.x * tx + f.y * ty + f.z * tz;
}

// Hauteur prêtée aux matériaux sans texture (milieu de [0, 1]).
static const float FLAT_MATERIAL_HEIGHT = 0.5;

// Surface du matériau `id` au point `s` (rgb = albédo, a = hauteur du relief), de
// couleur d'ensemble celle de la table sous `precipitation` (teinte d'humidité).
// Texturé : `texel / moyenne` ne garde que le détail (rapport à la moyenne, contraste
// intact) ; au loin, les mipmaps tendent vers la moyenne → couleur de la table seule.
float4 surfaceSample(uint id, SurfacePoint s, float3 w, float precipitation) {
  Material m = materials[id];
  float3 tint = materialAlbedo(id, precipitation);
  if (m.textureParams.x < 0.0)
    return float4(tint, FLAT_MATERIAL_HEIGHT);
  float4 texel = triplanarSample(uint(m.textureParams.x), m.textureParams.y, s, w);
  return float4(texel.rgb * m.textureInvMean.rgb * tint, texel.a);
}

// Matériaux portés par un sommet (3, cf. `VOXEL_MATERIALS`).
static const int VERTEX_MATERIALS = 3;
// Matériaux distincts au plus dans un triangle : 3 sommets × 3.
static const int PIXEL_MATERIALS = 3 * VERTEX_MATERIALS;
// Poids sous lequel un matériau n'est pas lu (3 lectures de texture épargnées).
static const float MIN_PIXEL_WEIGHT = 0.02;
// Matériaux lus au plus par pixel (les plus lourds) : leurs lectures sont gardées en
// registres le temps du mélange par hauteur. ≤ 4 (`heightBlendFactors`).
static const int BLEND_MATERIALS = 4;
// Profondeur du mélange par hauteur entre matériaux (cf. `heightBlendFactors`).
static const float HEIGHT_BLEND_DEPTH = 0.2;

// Albédo du mélange de matériaux au pixel. Poids du matériau m :
//   Σ_sommets λ_v · w_v(m)   (λ = barycentriques, w_v(m) = 0 si v ne porte pas m),
// soit l'interpolation de ses poids à travers le triangle — ce que faisait le
// Gouraud par sommet, mais matériau par matériau. `vIds` / `vWeights` : matériaux des
// 3 sommets (lus dans le point d'entrée : Slang n'accepte `GetAttributeAtVertex` que
// sur ses entrées directes).
float3 blendedAlbedo(uint4 vIds[3], float4 vWeights[3], float3 bary, float rain,
                     SurfacePoint s, float3 w) {
  uint ids[PIXEL_MATERIALS];
  float weights[PIXEL_MATERIALS];
  int count = 0;
  for (int v = 0; v < 3; v++) {
    for (int i = 0; i < VERTEX_MATERIALS; i++) {
      float wi = bary[v] * vWeights[v][i];
      if (wi <= 0.0)
        continue;
      // Fusion des doublons (un matériau porté par plusieurs sommets).
      int j = 0;
      while (j < count && ids[j] != vIds[v][i])
        j++;
      if (j == count) {
        ids[count] = vIds[v][i];
        weights[count] = 0.0;
        count++;
      }
      weights[j] += wi;
    }
  }

  // Les BLEND_MATERIALS plus lourds, hors négligeables (tri par insertion).
  // utilisé pour raison de performance (4*4 registres au lieu de 9 * 4)
  // trop compliqué pour ce que ca rapporte; à modifier ou suppriemr plus tard.
  uint topIds[BLEND_MATERIALS];
  float topWeights[BLEND_MATERIALS];
  int n = 0;
  for (int j = 0; j < count; j++) {
    if (weights[j] < MIN_PIXEL_WEIGHT)
      continue;
    int k = min(n, BLEND_MATERIALS - 1);
    if (n == BLEND_MATERIALS && weights[j] <= topWeights[k])
      continue; // liste pleine, plus léger que le dernier
    while (k > 0 && topWeights[k - 1] < weights[j]) {
      topIds[k] = topIds[k - 1];
      topWeights[k] = topWeights[k - 1];
      k--;
    }
    topIds[k] = ids[j];
    topWeights[k] = weights[j];
    n = min(n + 1, BLEND_MATERIALS);
  }

  // Mélange par hauteur (entrées au-delà de n : poids nul → absentes).
  float4 surfaces[BLEND_MATERIALS];
  float4 heights = 0.0, blendWeights = 0.0;
  for (int j = 0; j < n; j++) {
    surfaces[j] = surfaceSample(topIds[j], s, w, rain);
    heights[j] = surfaces[j].a;
    blendWeights[j] = topWeights[j];
  }
  float4 f = heightBlendFactors(heights, blendWeights, HEIGHT_BLEND_DEPTH);
  float3 albedo = 0.0;
  for (int j = 0; j < n; j++)
    albedo += f[j] * surfaces[j].rgb;
  return albedo;
}
