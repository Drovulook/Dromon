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

// Transmission de l'eau sous laquelle le fond marin n'est plus ombré : invisible en
// 8 bits (1/255 ≈ 4e-3), marge pour l'exposition du tone mapping.
static const float MIN_SEABED_VISIBILITY = 1e-3;

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
  float3 L = normalize(-ubo.lightDirection.xyz);
  float NdotL = max(dot(N, L), 0.0);
  float3 lightCol = ubo.lightColor.rgb * ubo.lightColor.w;
  float3 skyAmbient = ubo.ambient.rgb; // varie avec l'heure (day_night.rs)

  // Hors branche : dérivées et attributs des sommets (cf. `SurfacePoint`).
  SurfacePoint s = { vertIn.worldPos, ddx(vertIn.worldPos), ddy(vertIn.worldPos) };
  uint4 vIds[3];
  float4 vWeights[3];
  for (int v = 0; v < 3; v++) {
    vIds[v] = GetAttributeAtVertex(vertIn.materialIds, v);
    vWeights[v] = GetAttributeAtVertex(vertIn.materialWeights, v);
  }

  // Fond marin noyé (transmission sous MIN_SEABED_VISIBILITY) : sa couleur n'atteint
  // plus l'œil, ni matériaux ni éclairage ; `underwater` n'en garde que l'eau diffusée.
  float3 color = 0.0;
  float shadow = 1.0;
  bool shadowDone = false;
  float3 seabed = seabedTransmittance(vertIn.worldPos);
  if (max(seabed.r, max(seabed.g, seabed.b)) >= MIN_SEABED_VISIBILITY) {
    // Albédo du mélange de matériaux, texturé (triplanaire) ou couleur de table. Pas
    // de teinte d'humidité sur les textures pour l'instant.
    float3 material = blendedAlbedo(vIds, vWeights, bary, vertIn.rain, s, triplanarWeights(N));
    shadow = computeShadow(vertIn.worldPos, N, NdotL);
    shadowDone = true;
    // Ambiant = lumière indirecte du ciel, légèrement bleutée. MULTIPLIÉ par la
    // couleur du matériau (et non ajouté) : les zones à l'ombre gardent leur teinte
    // (vert foncé naturel) au lieu d'être délavées vers un gris blanchâtre.
    // Ambiant hémisphérique : une face voit d'autant plus de ciel qu'elle est tournée
    // vers le haut ; le reste de son hémisphère voit le sol, plus sombre.
    float skyFactor = saturate(N.z); // 1 = face au ciel, 0 = face au sol; on peut aussi utiliser N.z * 0.5 + 0.5
    float3 ambient = lerp(skyAmbient * ubo.ambient.w, skyAmbient, skyFactor);
    color = material * (ambient + lightCol * NdotL * shadow);
  }

  // Sous la mer : absorbé et noyé dans la couleur de l'eau.
  color = underwater(color, vertIn.worldPos);

  // Écume du rivage : flotte à la surface, donc ni absorbée ni teintée par l'eau.
  // Éclairée comme une face horizontale (ombre du fond, proche en eau peu profonde).
  float foam = shoreFoam(vertIn.worldPos);
  if (foam > 0.0) {
    if (!shadowDone)
      shadow = computeShadow(vertIn.worldPos, N, NdotL);
    float3 foamColor = foamAlbedo() * (skyAmbient + lightCol * max(L.z, 0.0) * shadow);
    color = lerp(color, foamColor, foam);
  }

  // Brouillard sur la partie aérienne du regard seulement. Caméra et fragment sous
  // l'eau : aucun air traversé.
  if (!(cameraUnderSea() && vertIn.worldPos.z < seaLevel()))
    color = applyFog(color, airPathEnd(vertIn.worldPos), L, lightCol);

  return float4(tonemap(color), 1.0);
}
