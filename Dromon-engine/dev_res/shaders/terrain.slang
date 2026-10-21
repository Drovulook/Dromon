// Shader du terrain volumétrique (géométrie issue de Surface Nets).

#include "lib/shadow.slang"
#include "lib/atmosphere.slang"
#include "lib/water.slang"
#include "lib/materials.slang"
#include "lib/tonemap.slang"

// Entrées vertex : doivent matcher `TerrainVertex` côté Rust (locations 0 à 3).
struct VSInput {
  float3 inPosition;  // location 0
  float3 inNormal;    // location 1 — normale lissée (gradient de densité)
  uint4 inMaterials;  // location 2 — IDs dans la table des matériaux (w inutilisé)
  float4 inWeights;   // location 3 — poids associés, somme 1 (w inutilisé)
}

// Matériaux portés par un sommet (3, cf. `VOXEL_MATERIALS`).
static const int VERTEX_MATERIALS = 3;

// Humidité côtière ajoutée aux précipitations pour la **teinte** des matériaux (sable
// de plage beige, frange côtière plus verte) : pleine sur la côte, nulle à `range`
// voxels dans les terres (material_rules.ron → `coast_moisture`).
float coastMoisture(float2 xy) {
  float coast = 1.0 - smoothstep(0.0, ubo.materialTint.y, coastDistance(xy));
  return ubo.materialTint.x * coast;
}

struct VSOutput {
  float4 pos : SV_Position;
  float3 worldPos;
  float3 worldNormal; // normale monde, interpolée pour l'éclairage lisse
  // Précipitations + humidité côtière, pour la teinte. Varient au km : une lecture
  // par sommet suffit (cf. vault, Generation-Monde).
  float rain;
  // Matériaux du sommet, **non interpolés** : le fragment lit ceux des 3 sommets de
  // son triangle (`GetAttributeAtVertex`) et les mélange lui-même.
  nointerpolation uint4 materialIds;
  nointerpolation float4 materialWeights;
}

[shader("vertex")]
VSOutput terrainVertMain(VSInput input) {
  VSOutput output;
  float4 worldPos = mul(pc.model, float4(input.inPosition, 1.0));
  output.pos = mul(ubo.proj, mul(ubo.view, worldPos));
  // model = identité pour le terrain, mais on applique la normalMatrix par
  // cohérence (gère un éventuel scale non uniforme).
  output.worldNormal = mul((float3x3)pc.normalMatrix, input.inNormal);
  output.rain = precipitation(worldPos.xy) + coastMoisture(worldPos.xy);
  output.materialIds = input.inMaterials;
  output.materialWeights = input.inWeights;
  output.worldPos = worldPos.xyz;
  return output;
}

// Matériaux distincts au plus dans un triangle : 3 sommets × 3.
static const int PIXEL_MATERIALS = 3 * VERTEX_MATERIALS;
// Poids sous lequel un matériau n'est pas lu (3 lectures de texture épargnées).
static const float MIN_PIXEL_WEIGHT = 0.02;
// Matériaux lus au plus par pixel (les plus lourds) : leurs lectures sont gardées en
// registres le temps du mélange par hauteur.
static const int BLEND_MATERIALS = 4;
// Profondeur du mélange par hauteur : écart de score sous le meilleur en deçà duquel
// un matériau contribue encore. → 0 : frontière découpée ; grand : moyenne pondérée.
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

  // Mélange par hauteur : score = hauteur du relief + poids. Seuls les matériaux à
  // moins de HEIGHT_BLEND_DEPTH du meilleur score contribuent : le relief le plus
  // haut l'emporte au pixel (frontière irrégulière, pas de surimpression).
  float4 surfaces[BLEND_MATERIALS];
  float best = -1.0;
  for (int j = 0; j < n; j++) {
    surfaces[j] = surfaceSample(topIds[j], s, w, rain);
    best = max(best, surfaces[j].a + topWeights[j]);
  }
  float3 albedo = 0.0;
  float total = 0.0;
  for (int j = 0; j < n; j++) {
    float b = max(surfaces[j].a + topWeights[j] - best + HEIGHT_BLEND_DEPTH, 0.0);
    albedo += b * surfaces[j].rgb;
    total += b;
  }
  return total > 0.0 ? albedo / total : 0.0;
}

// Vertex shader « depth-only » de la passe d'ombre. Entrée minimale (position
// seule) → la pipeline d'ombre du terrain n'exige que la location 0.
struct ShadowVSInput {
  float3 inPosition; // location 0
}
[shader("vertex")]
float4 terrainShadowVertMain(ShadowVSInput input) : SV_Position {
  float4 worldPos = mul(pc.model, float4(input.inPosition, 1.0));
  return mul(ubo.lightViewProj, worldPos);
}

[shader("fragment")]
float4 terrainFragMain(VSOutput vertIn, float3 bary : SV_Barycentrics) : SV_Target {
  // Normale lissée interpolée depuis les sommets (gradient de densité). Donne un
  // ombrage doux sur la surface Surface Nets, contrairement au flat shading.
  // La renormaliser : l'interpolation linéaire raccourcit le vecteur.
  float3 N = normalize(vertIn.worldNormal);

  // Albédo du mélange de matériaux, texturé (triplanaire) ou couleur de table. Pas
  // de teinte d'humidité sur les textures pour l'instant.
  SurfacePoint s = { vertIn.worldPos, ddx(vertIn.worldPos), ddy(vertIn.worldPos) };
  uint4 vIds[3];
  float4 vWeights[3];
  for (int v = 0; v < 3; v++) {
    vIds[v] = GetAttributeAtVertex(vertIn.materialIds, v);
    vWeights[v] = GetAttributeAtVertex(vertIn.materialWeights, v);
  }
  float3 material = blendedAlbedo(vIds, vWeights, bary, vertIn.rain, s, triplanarWeights(N));

  float3 L = normalize(-ubo.lightDirection.xyz);
  float NdotL = max(dot(N, L), 0.0);

  float shadow = computeShadow(vertIn.worldPos, N, NdotL);
  float3 lightCol = ubo.lightColor.rgb * ubo.lightColor.w;
  // Ambiant = lumière indirecte du ciel, légèrement bleutée. MULTIPLIÉ par la
  // couleur du matériau (et non ajouté) : les zones à l'ombre gardent leur teinte
  // (vert foncé naturel) au lieu d'être délavées vers un gris blanchâtre.
  // Ambiant hémisphérique : une face voit d'autant plus de ciel qu'elle est tournée
  // vers le haut ; le reste de son hémisphère voit le sol, plus sombre.
  float skyFactor = saturate(N.z); // 1 = face au ciel, 0 = face au sol; on peut aussi utiliser N.z * 0.5 + 0.5
  float3 skyAmbient = ubo.ambient.rgb; // varie avec l'heure (day_night.rs)
  float3 ambient = lerp(skyAmbient * ubo.ambient.w, skyAmbient, skyFactor);
  float3 color = material * (ambient + lightCol * NdotL * shadow);

  // Sous la mer : absorbé et noyé dans la couleur de l'eau.
  color = underwater(color, vertIn.worldPos);

  // Écume du rivage : flotte à la surface, donc ni absorbée ni teintée par l'eau.
  // Éclairée comme une face horizontale (ombre du fond, proche en eau peu profonde).
  float foam = shoreFoam(vertIn.worldPos);
  float3 foamColor = foamAlbedo() * (skyAmbient + lightCol * max(L.z, 0.0) * shadow);
  color = lerp(color, foamColor, foam);

  // Brouillard sur la partie aérienne du regard seulement. Caméra et fragment sous
  // l'eau : aucun air traversé.
  if (!(cameraUnderSea() && vertIn.worldPos.z < seaLevel()))
    color = applyFog(color, airPathEnd(vertIn.worldPos), L, lightCol);

  return float4(tonemap(color), 1.0);
}
