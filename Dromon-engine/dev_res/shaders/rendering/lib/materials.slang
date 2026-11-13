// Table des matériaux (set 4) : SSBO indexé par ID (0 = air, puis materials.ron dans
// l'ordre, puis le matériau de debug). ⚠ Disposition synchronisée avec `GpuMaterial`
// (material_buffer.rs).
#pragma once
#include "bindings.slang"
#include "common/noise.slang"

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

// Réglages des textures du terrain (material_textures.ron, cf. MaterialTexturesConfig).
float triplanarSharpness() { return ubo.textureBlend.x; }
float projectionBlendDepth() { return ubo.textureBlend.y; }
float materialBlendDepth() { return ubo.textureBlend.z; }
float normalStrength() { return ubo.textureDetail.x; }
float textureAoStrength() { return ubo.textureDetail.y; }
// Hauteur de relief prêtée aux matériaux sans texture.
float flatMaterialHeight() { return ubo.textureDetail.z; }
float warpAmplitude() { return ubo.textureWarp.x; }
float warpWavelength() { return ubo.textureWarp.y; }
float tintWavelength() { return ubo.textureTint.x; }
float brightnessVariation() { return ubo.textureTint.y; }
float hueVariation() { return ubo.textureTint.z; }

// 2ᵉ octave du warp : longueur d'onde et amplitude ÷ 3 (même étirement que la 1ʳᵉ).
static const float WARP_OCTAVE_RATIO = 3.0;

// Décalage des lectures de texture (domain warping) : casse l'alignement des copies
// d'une texture répétée. 2 octaves de bruit centré, |décalage| ≤ A·(1 + 1/3).
float3 textureWarp(float3 pos) {
  if (warpAmplitude() <= 0.0)
    return 0.0;
  float3 p = pos / warpWavelength();
  float3 n = (valueNoise3(p, 1u) - 0.5)
           + (valueNoise3(p * WARP_OCTAVE_RATIO, 2u) - 0.5) / WARP_OCTAVE_RATIO;
  return 2.0 * warpAmplitude() * n;
}

// Variation de couleur à grande échelle (multiplicateur, moyenne 1) : l'œil ne compare
// plus des copies identiques. Luminosité, puis balance rouge/bleu (chaud/froid).
float3 macroTint(float3 pos) {
  float2 n = valueNoise3(pos / tintWavelength(), 3u).xy * 2.0 - 1.0;
  float h = n.y * hueVariation();
  return (1.0 + n.x * brightnessVariation()) * float3(1.0 + h, 1.0, 1.0 - h);
}

// Poids sous lequel une projection n'est pas lue : sur un sol plat ou une paroi, une
// seule lecture au lieu de 3.
static const float MIN_PROJECTION_WEIGHT = 0.01;

// Poids des projections selon X, Y, Z pour la normale `N` : |N|^k normalisé, somme 1,
// les négligeables mis à 0 (puis renormalisé).
float3 triplanarWeights(float3 N) {
  float3 w = pow(abs(N), triplanarSharpness());
  w /= w.x + w.y + w.z;
  w = select(w < MIN_PROJECTION_WEIGHT, 0.0, w);
  return w / (w.x + w.y + w.z);
}

// Position monde et ses dérivées écran, pour lire les textures avec des gradients
// explicites : les lectures ont lieu dans des branches qui varient d'un pixel à
// l'autre, où les dérivées implicites (différences dans le quad 2×2) sont indéfinies.
struct SurfacePoint {
  float3 pos; // décalé par `textureWarp` dans `blendedSurface`, si une texture est lue
  float3 dx;  // ddx(pos), calculé hors de toute branche
  float3 dy;  // ddy(pos)
};

// Point de lecture des textures pour la position monde `pos`. Dérivées de `pos` sans
// celles du warp : son étirement (≲ 25 %) change à peine le niveau de mipmap.
SurfacePoint surfacePoint(float3 pos, float3 dx, float3 dy) {
  SurfacePoint s = { pos, dx, dy };
  return s;
}

// Répétitions de texture par pixel au-delà desquelles la texture n'est plus lue
// (couleur de table seule) : son détail y est indiscernable. Réglé à l'œil.
static const float TEXTURE_CUTOFF = 0.15;

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

// Surface d'un matériau en un point.
struct Surface {
  float3 albedo; // linéaire
  float height;  // relief, pour le mélange par hauteur
  float3 normal; // monde, pas forcément unitaire
  float ao;
};

// Normale d'espace tangent d'un texel de `terrainNormals` (rg = x, y ; z reconstruit,
// la normale étant unitaire). normal.png est en convention OpenGL (vert = haut de
// l'image) ; en Vulkan, v croît vers le bas de l'image → vert inversé.
float3 tangentNormal(float2 rg) {
  float2 xy = (rg * 2.0 - 1.0) * float2(normalStrength(), -normalStrength());
  return float3(xy, sqrt(saturate(1.0 - dot(xy, xy))));
}

// Surface triplanaire (albédo non teinté) de la couche `layer`, à `scale` répétitions
// par unité monde. Projection selon X → plan YZ, etc. ; Z = haut. `N` = normale
// géométrique, `w` = ses poids triplanaires.
Surface triplanarSample(uint layer, float scale, SurfacePoint s, float3 N, float3 w) {
  float3 p = s.pos * scale, dx = s.dx * scale, dy = s.dy * scale;
  // Projections de poids nul sautées (cf. `triplanarWeights`). c = albédo + hauteur,
  // n = normale + AO.
  float4 cx = 0.0, cy = 0.0, cz = 0.0, nx = 0.0, ny = 0.0, nz = 0.0;
  if (w.x > 0.0) {
    cx = terrainTextures.SampleGrad(float3(p.yz, layer), dx.yz, dy.yz);
    nx = terrainNormals.SampleGrad(float3(p.yz, layer), dx.yz, dy.yz);
  }
  if (w.y > 0.0) {
    cy = terrainTextures.SampleGrad(float3(p.xz, layer), dx.xz, dy.xz);
    ny = terrainNormals.SampleGrad(float3(p.xz, layer), dx.xz, dy.xz);
  }
  if (w.z > 0.0) {
    cz = terrainTextures.SampleGrad(float3(p.xy, layer), dx.xy, dy.xy);
    nz = terrainNormals.SampleGrad(float3(p.xy, layer), dx.xy, dy.xy);
  }
  float4 f = heightBlendFactors(float4(cx.a, cy.a, cz.a, 0.0), float4(w, 0.0),
                                projectionBlendDepth());

  // Normale de détail : le relief projeté et le terrain sont deux champs de hauteur sur
  // le plan de projection → leurs pentes s'additionnent (exact).
  // Pour worldX, (u, v, w) = (Y, Z, X) : n = ( N.y/N.x + t.x/t.z ,  N.z/N.x + t.y/t.z ,  1 )
  // soit, au dénominateur commun |N.x|·t.z :
  //   n = ( N.y·t.z + t.x·|N.x| ,  N.z·t.z + t.y·|N.x| ,  N.x·t.z ) = t.z·N + |N.x|·(0, t.x, t.y)
  // |N.x| : la face regarde vers ±X (avec N.x seul, relief inversé sur les faces −X).
  // On pourrait aussi utiliser le whiteout blending (t.z ≈ 1 et |N.x| ≈ 1 sur u, v) :
  //   n_whiteout = ( N.y + t.x ,  N.z + t.y ,  N.x·t.z )   → relief un peu accentué.
  float3 tx = tangentNormal(nx.rg), ty = tangentNormal(ny.rg), tz = tangentNormal(nz.rg);
  float3 worldX = tx.z * N + abs(N.x) * float3(0.0, tx.x, tx.y);
  float3 worldY = ty.z * N + abs(N.y) * float3(ty.x, 0.0, ty.y);
  float3 worldZ = tz.z * N + abs(N.z) * float3(tz.x, tz.y, 0.0);

  float4 c = f.x * cx + f.y * cy + f.z * cz;
  Surface r;
  r.albedo = c.rgb;
  r.height = c.a;
  r.normal = f.x * worldX + f.y * worldY + f.z * worldZ;
  r.ao = f.x * nx.b + f.y * ny.b + f.z * nz.b;
  return r;
}

// Surface du matériau `id` au point `s`, de couleur d'ensemble celle de la table sous
// `precipitation` (teinte d'humidité). Texturé : `texel / moyenne` ne garde que le
// détail (rapport à la moyenne, contraste intact) ; au loin, les mipmaps tendent vers
// la moyenne → couleur de la table seule. Sans texture, ou `textured` faux (détail
// indiscernable) : aucune lecture, normale géométrique `N`.
Surface surfaceSample(uint id, SurfacePoint s, float3 N, float3 w, float precipitation,
                      bool textured) {
  Material m = materials[id];
  float3 tint = materialAlbedo(id, precipitation);
  if (!textured) {
    Surface flat = { tint, flatMaterialHeight(), N, 1.0 };
    return flat;
  }
  Surface r = triplanarSample(uint(m.textureParams.x), m.textureParams.y, s, N, w);
  r.albedo *= m.textureInvMean.rgb * tint;
  return r;
}

// Matériaux portés par un sommet (3, cf. `VOXEL_MATERIALS`).
static const int VERTEX_MATERIALS = 3;
// Emplacement de matériau vide (ID impossible).
static const uint NO_MATERIAL = 0xFFFFFFFFu;
// Poids sous lequel un matériau n'est pas lu (3 lectures de texture épargnées).
static const float MIN_PIXEL_WEIGHT = 0.02;
// Matériaux lus au plus par pixel (les plus lourds) : leurs lectures sont gardées en
// registres le temps du mélange par hauteur. ≤ 4 (`heightBlendFactors`).
static const int BLEND_MATERIALS = 4;

// Surface du mélange de matériaux au pixel (normale unitaire). Poids du matériau m :
//   Σ_sommets λ_v · w_v(m)   (λ = barycentriques, w_v(m) = 0 si v ne porte pas m),
// soit l'interpolation de ses poids à travers le triangle — ce que faisait le
// Gouraud par sommet, mais matériau par matériau. `vIds` / `vWeights` : matériaux des
// 3 sommets (lus dans le point d'entrée : Slang n'accepte `GetAttributeAtVertex` que
// sur ses entrées directes). `N` = normale géométrique, `w` = ses poids triplanaires.
// `allowTextures` faux : aucune lecture de texture (fond sous trop d'eau).
Surface blendedSurface(uint4 vIds[3], float4 vWeights[3], float3 bary, float rain,
                       SurfacePoint s, float3 N, float3 w, bool allowTextures) {
  // Poids cumulés dans BLEND_MATERIALS emplacements fixes. Boucles déroulées : indices
  // constants, tout reste en registres (la recherche de doublons par index variable
  // coûtait ~12 % du shader). Exact jusqu'à 4 matériaux distincts ; au-delà, un
  // nouveau venu évince le plus léger s'il pèse plus (poids partiels : approché).
  uint4 slotIds = NO_MATERIAL;
  float4 slotWeights = 0.0;
  [ForceUnroll]
  for (int v = 0; v < 3; v++) {
    [ForceUnroll]
    for (int i = 0; i < VERTEX_MATERIALS; i++) {
      uint id = vIds[v][i];
      float wi = bary[v] * vWeights[v][i];
      bool4 match = slotIds == id;
      slotWeights += select(match, float4(wi), float4(0.0));
      // Absent : emplacement le plus léger (vide = poids 0), s'il pèse moins que wi.
      float lightest = min(min(slotWeights.x, slotWeights.y), min(slotWeights.z, slotWeights.w));
      bool place = !any(match) && wi > lightest;
      [ForceUnroll]
      for (int k = 0; k < BLEND_MATERIALS; k++) {
        bool take = place && slotWeights[k] == lightest;
        slotIds[k] = take ? id : slotIds[k];
        slotWeights[k] = take ? wi : slotWeights[k];
        place = place && !take;
      }
    }
  }
  // Négligeables retirés : poids nul = emplacement absent (cf. `heightBlendFactors`).
  slotWeights = select(slotWeights < MIN_PIXEL_WEIGHT, float4(0.0), slotWeights);

  // Matériaux dont la texture est lue : texturés, et pixel assez petit devant une
  // répétition (petit axe de l'empreinte : l'anisotrope choisit la mipmap selon lui).
  // Le warp ne sert qu'aux lectures : calculé seulement si l'une d'elles a lieu. La
  // teinte lit la position non décalée, sinon elle sauterait au seuil.
  float footprint = min(length(s.dx), length(s.dy));
  bool textured[BLEND_MATERIALS] = { false, false, false, false };
  bool anyTextured = false;
  if (allowTextures) {
    [ForceUnroll]
    for (int j = 0; j < BLEND_MATERIALS; j++) {
      if (slotWeights[j] > 0.0) {
        float4 params = materials[slotIds[j]].textureParams;
        textured[j] = params.x >= 0.0 && footprint * params.y < TEXTURE_CUTOFF;
        anyTextured = anyTextured || textured[j];
      }
    }
  }
  float3 tintPos = s.pos;
  if (anyTextured)
    s.pos += textureWarp(s.pos);

  // Mélange par hauteur (emplacements absents : poids nul, ignorés).
  Surface none = { float3(0.0), 0.0, float3(0.0), 0.0 };
  Surface surfaces[BLEND_MATERIALS];
  float4 heights = 0.0;
  [ForceUnroll]
  for (int j = 0; j < BLEND_MATERIALS; j++) {
    surfaces[j] = none;
    if (slotWeights[j] > 0.0)
      surfaces[j] = surfaceSample(slotIds[j], s, N, w, rain, textured[j]);
    heights[j] = surfaces[j].height;
  }
  float4 f = heightBlendFactors(heights, slotWeights, materialBlendDepth());
  Surface r = none;
  [ForceUnroll]
  for (int j = 0; j < BLEND_MATERIALS; j++) {
    r.albedo += f[j] * surfaces[j].albedo;
    r.height += f[j] * surfaces[j].height;
    r.normal += f[j] * surfaces[j].normal;
    r.ao += f[j] * surfaces[j].ao;
  }
  r.normal = any(slotWeights > 0.0) ? normalize(r.normal) : N;
  r.albedo *= macroTint(tintPos);
  return r;
}
