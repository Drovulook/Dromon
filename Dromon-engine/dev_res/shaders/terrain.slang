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
  float3 color;       // couleur interpolée entre les sommets par le rasterizer
}

[shader("vertex")]
VSOutput terrainVertMain(VSInput input) {
  VSOutput output;
  float4 worldPos = mul(pc.model, float4(input.inPosition, 1.0));
  output.pos = mul(ubo.proj, mul(ubo.view, worldPos));
  // model = identité pour le terrain, mais on applique la normalMatrix par
  // cohérence (gère un éventuel scale non uniforme).
  output.worldNormal = mul((float3x3)pc.normalMatrix, input.inNormal);
  // Albédo du sommet, interpolé ensuite (Gouraud) : pour une grandeur linéaire comme
  // l'albédo, identique à interpoler les poids. Les précipitations varient au km : une
  // lecture par sommet suffit (cf. vault, Generation-Monde).
  float rain = precipitation(worldPos.xy) + coastMoisture(worldPos.xy);
  float3 albedo = 0.0;
  for (int i = 0; i < VERTEX_MATERIALS; i++)
    albedo += input.inWeights[i] * materialAlbedo(input.inMaterials[i], rain);
  output.color = albedo;
  output.worldPos = worldPos.xyz;
  return output;
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
float4 terrainFragMain(VSOutput vertIn) : SV_Target {
  // Normale lissée interpolée depuis les sommets (gradient de densité). Donne un
  // ombrage doux sur la surface Surface Nets, contrairement au flat shading.
  // La renormaliser : l'interpolation linéaire raccourcit le vecteur.
  float3 N = normalize(vertIn.worldNormal);

  // Albédo du mélange de matériaux, calculé par sommet (table GPU) et interpolé par
  // le rasterizer.
  float3 material = vertIn.color;

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
