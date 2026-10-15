// Surface de la mer (CDLOD) : une grille unique redessinée par nœud, placée par
// push constants, avec geomorphing entre niveaux, déplacée par la houle (Gerstner).
// Ne dessine que le reflet, pondéré par
// Fresnel ; le fond vu à travers l'eau est teinté dans terrain.slang.

#include "lib/atmosphere.slang"
#include "lib/water.slang"

// ⚠ Même disposition que `WaterPushConstants` (world/water/mod.rs).
struct WaterPushConstants {
  float2 origin;     // coin (x min, y min) du nœud, unités monde
  float cellSize;    // côté d'une cellule de la grille
  float seaLevel;
  float morphStart;  // distance caméra → sommet où k quitte 0…
  float morphEnd;    // …et atteint 1 (forme du niveau au-dessus)
};
[[vk::push_constant]]
ConstantBuffer<WaterPushConstants> waterPc;

struct WaterVSInput {
  float2 gridPos; // location 0 — (i, j) dans la grille
}

struct WaterVSOutput {
  float4 pos : SV_Position;
  float3 worldPos;
  float2 gridXY;            // point de grille non déplacé, où évaluer les normales
}

[shader("vertex")]
WaterVSOutput waterVertMain(WaterVSInput input) {
  float2 grid = input.gridPos;

  // k depuis la position monde AVANT déplacement : deux nœuds voisins calculent le
  // même k sur leur bord commun. Même distance (au plan plat) que le quadtree.
  float3 flatPos = float3(waterPc.origin + grid * waterPc.cellSize, waterPc.seaLevel);
  float dist = distance(flatPos, ubo.cameraPosition.xyz);
  float k = saturate((dist - waterPc.morphStart) / (waterPc.morphEnd - waterPc.morphStart));

  // Sommets impairs (1 sur l'axe concerné) glissés d'une cellule vers leur voisin
  // pair : à k = 1, la grille a la forme exacte de celle du niveau au-dessus.
  float2 odd = frac(grid * 0.5) * 2.0;
  float2 xy = waterPc.origin + (grid - odd * k) * waterPc.cellSize;

  // Vagues évaluées à `xy`, APRÈS le glissement : un sommet posé sur son voisin en
  // prend la hauteur. Espacement réel des sommets : `cellSize` à k = 0, double à k = 1
  // — continu d'un nœud à son voisin (fin à k = 1 = grossier à k = 0).
  float3 offset = gerstnerOffset(xy, waterPc.cellSize * (1.0 + k));
  float3 worldPos = float3(xy, waterPc.seaLevel) + offset;

  WaterVSOutput output;
  output.pos = mul(ubo.proj, mul(ubo.view, float4(worldPos, 1.0)));
  output.worldPos = worldPos;
  output.gridXY = xy;
  return output;
}

[shader("fragment")]
float4 waterFragMain(WaterVSOutput vertIn) : SV_Target {
  // Variation de `gridXY` d'un pixel au voisin (dérivées d'écran). AVANT le discard :
  // les dérivées sont indéfinies si un voisin du quad 2×2 a été éliminé.
  float2 dx = ddx(vertIn.gridXY);
  float2 dy = ddy(vertIn.gridXY);

  // Pas d'eau au-delà du terrain chargé (elle recouvrirait les terres absentes), ni
  // sur terre. Même masque que l'absorption du fond.
  if (!seaCovers(vertIn.worldPos.xy))
    discard;

  float3 V = normalize(vertIn.worldPos - ubo.cameraPosition.xyz);
  float3 L = normalize(-ubo.lightDirection.xyz);
  float3 lightCol = ubo.lightColor.rgb * ubo.lightColor.w;

  // Normale des vagues au pixel, retournée vers la caméra si elle est sous l'eau.
  SeaNormal waves = gerstnerNormal(vertIn.gridXY, dx, dy);
  bool fromBelow = cameraUnderSea();
  float3 N = waves.normal * (fromBelow ? -1.0 : 1.0);
  float F = fresnel(saturate(dot(N, -V)));

  // Vu d'en dessous (provisoire, sans fenêtre de Snell) : reflet des profondeurs, pas
  // d'air traversé.
  if (fromBelow)
    return float4(F * deepWaterColor(vertIn.worldPos.xy), F);

  float3 R = reflect(V, N);
  R.z = max(R.z, 0.0); // une pente forte peut réfléchir vers le bas : horizon au plus bas
  R = normalize(R);
  float3 sky = airColor(R, L, lightCol);
  // Contient déjà son Fresnel. Pas d'ombre portée : un relief entre le soleil et l'eau
  // n'éteint pas le reflet. Rides + vagues plus petites qu'un pixel.
  float slopeVariance = ubo.waterDeepColor.w + waves.lostSlopeVariance;
  float3 glint = sunGlint(N, -V, L, lightCol, slopeVariance);

  // Premultiplied alpha (cf. pipelines/water.rs) : sortie = F·ciel + reflet du soleil,
  // α = F. Le brouillard s'applique au tout, factorisé par F : le fond (déjà dans le
  // framebuffer) l'a reçu jusqu'à ce même point, le mélange reste cohérent.
  float3 reflection = applyFog(sky + glint / F, vertIn.worldPos, L, lightCol);
  return float4(F * reflection, F);
}
