/// Interpolation lisse (Hermite) : 0 sous `edge0`, 1 au-dessus de `edge1`, courbe
/// en S entre les deux. Équivalent du `smoothstep` des shaders.
pub(super) fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Variante C2 de [`smoothstep`] (polynôme quintique de Ken Perlin,
/// `6t⁵ − 15t⁴ + 10t³`) : non seulement la pente, mais aussi la **courbure**
/// s'annule aux deux bords. Pas de saut de dérivée seconde aux seuils → évite le
/// liseré d'éclairage (bande de Mach) que produit `smoothstep` quand on module
/// une grandeur visible le long d'une iso-courbe.
pub(super) fn smootherstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}
