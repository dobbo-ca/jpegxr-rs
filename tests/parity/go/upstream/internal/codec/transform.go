// Ported from jxrlib strInvTransform.c, strTransform.c (BSD-2, see NOTICE).

package codec

// Every kernel takes one channel's two-row buffer and indices into it.

func strDCT2x2dn(p []int32, ia, ib, ic, id int) {
	a, b, C, d := p[ia], p[ib], p[ic], p[id]
	a += d
	b -= C
	t := (a - b) >> 1
	c := t - d
	d = t - C
	a -= d
	b += c
	p[ia], p[ib], p[ic], p[id] = a, b, c, d
}

func strDCT2x2up(p []int32, ia, ib, ic, id int) {
	a, b, C, d := p[ia], p[ib], p[ic], p[id]
	a += d
	b -= C
	t := (a - b + 1) >> 1
	c := t - d
	d = t - C
	a -= d
	b += c
	p[ia], p[ib], p[ic], p[id] = a, b, c, d
}

func strDCT2x2dnDec(p []int32, ia, ib, ic, id int) {
	a, b, C, d := p[ia], p[ib], p[ic], p[id]
	a += d
	b -= C
	t := (a - b) >> 1
	c := t - d
	d = t - C
	a -= d
	b += c
	p[ia], p[ib], p[ic], p[id] = a*2, b*2, c*2, d*2
}

func fourButterflyHardcoded1(p []int32, o int) {
	strDCT2x2dn(p, o+0, o+4, o+8, o+12)
	strDCT2x2dn(p, o+1, o+5, o+9, o+13)
	strDCT2x2dn(p, o+2, o+6, o+10, o+14)
	strDCT2x2dn(p, o+3, o+7, o+11, o+15)
}

func irotate1(p []int32, ia, ib int) {
	p[ia] -= (p[ib] + 1) >> 1
	p[ib] += (p[ia] + 1) >> 1
}

func invOdd(p []int32, ia, ib, ic, id int) {
	a, b, c, d := p[ia], p[ib], p[ic], p[id]
	b += d
	a -= c
	d -= b >> 1
	c += (a + 1) >> 1
	a -= (b*3 + 4) >> 3
	b += (a*3 + 4) >> 3
	c -= (d*3 + 4) >> 3
	d += (c*3 + 4) >> 3
	c -= (b + 1) >> 1
	d = ((a + 1) >> 1) - d
	b += c
	a -= d
	p[ia], p[ib], p[ic], p[id] = a, b, c, d
}

func invOddOdd(p []int32, ia, ib, ic, id int) {
	a, b, c, d := p[ia], p[ib], p[ic], p[id]
	d += a
	c -= b
	t1 := d >> 1
	a -= t1
	t2 := c >> 1
	b += t2
	a -= (b*3 + 3) >> 3
	b += (a*3 + 3) >> 2
	a -= (b*3 + 4) >> 3
	b -= t2
	a += t1
	c += b
	d -= a
	p[ia], p[ib], p[ic], p[id] = a, -b, -c, d
}

func invOddOddPost(p []int32, ia, ib, ic, id int) {
	a, b, c, d := p[ia], p[ib], p[ic], p[id]
	d += a
	c -= b
	t1 := d >> 1
	a -= t1
	t2 := c >> 1
	b += t2
	a -= (b*3 + 6) >> 3
	b += (a*3 + 2) >> 2
	a -= (b*3 + 4) >> 3
	b -= t2
	a += t1
	c += b
	d -= a
	p[ia], p[ib], p[ic], p[id] = a, b, c, d
}

func strHSTdec1Alternate(p []int32, ia, id int) {
	a, d := p[ia], p[id]
	a += d
	d = (a >> 1) - d
	a += (d * 3) >> 3
	d += (a * 3) >> 4
	d += a >> 7
	d -= a >> 10
	p[ia], p[id] = a, d
}

func strHSTdec1Edge(a, d int32) (int32, int32) {
	a += d
	d = (a >> 1) - d
	a += (d * 3) >> 3
	d += (a * 3) >> 4
	d += a >> 7
	d -= a >> 10
	a += (d*3 + 4) >> 3
	d -= a >> 1
	a += d
	return a, -d
}

func strHSTdec(p []int32, ia, ib, ic, id int) {
	a, b, c, d := p[ia], p[ib], p[ic], p[id]
	b -= c
	a += (d*3 + 4) >> 3
	d -= b >> 1
	c = ((a - b) >> 1) - c
	p[ic] = d
	p[id] = c
	p[ia] = a - c
	p[ib] = b + d
}

func strPost2Alternate(p []int32, ia, ib int) {
	a, b := p[ia], p[ib]
	b += (a + 2) >> 2
	a += (b + 1) >> 1
	a += b >> 5
	a += b >> 9
	a += b >> 13
	b += (a + 2) >> 2
	p[ia], p[ib] = a, b
}

func strPost2x2Alternate(p []int32, ia, ib, ic, id int) {
	a, b, c, d := p[ia], p[ib], p[ic], p[id]
	a += d
	b += c
	d -= (a + 1) >> 1
	c -= (b + 1) >> 1
	b += (a + 2) >> 2
	a += (b + 1) >> 1
	a += b >> 5
	a += b >> 9
	a += b >> 13
	b += (a + 2) >> 2
	d += (a + 1) >> 1
	c += (b + 1) >> 1
	a -= d
	b -= c
	p[ia], p[ib], p[ic], p[id] = a, b, c, d
}

func strPost4Alternate(p []int32, ia, ib, ic, id int) {
	a, b, c, d := p[ia], p[ib], p[ic], p[id]
	a += d
	b += c
	d -= (a + 1) >> 1
	c -= (b + 1) >> 1
	a, d = strHSTdec1Edge(a, d)
	b, c = strHSTdec1Edge(b, c)
	c -= (d + 1) >> 1
	d += (c + 1) >> 1
	d += (a + 1) >> 1
	c += (b + 1) >> 1
	a -= d
	b -= c
	p[ia], p[ib], p[ic], p[id] = a, b, c, d
}

// strIDCT4x4Stage1 inverts one 4x4 block at o.
func strIDCT4x4Stage1(p []int32, o int) {
	strDCT2x2up(p, o+0, o+1, o+2, o+3)
	invOdd(p, o+5, o+4, o+7, o+6)
	invOdd(p, o+10, o+8, o+11, o+9)
	invOddOdd(p, o+15, o+14, o+13, o+12)
	fourButterflyHardcoded1(p, o)
}

// Pair k is blocks o+64k and o+64k+16.
func strIDCT4x4Stage1PairsGo(p []int32, o, n int) {
	for ; n > 0; n-- {
		strIDCT4x4Stage1(p, o)
		strIDCT4x4Stage1(p, o+16)
		o += 64
	}
}

func strIDCT4x4Stage2(p []int32, o int) {
	invOdd(p, o+32, o+48, o+96, o+112)
	invOdd(p, o+128, o+192, o+144, o+208)
	invOddOdd(p, o+160, o+224, o+176, o+240)
	strDCT2x2up(p, o+0, o+64, o+16, o+80)
	strDCT2x2dn(p, o+0, o+192, o+48, o+240)
	strDCT2x2dn(p, o+64, o+128, o+112, o+176)
	strDCT2x2dn(p, o+16, o+208, o+32, o+224)
	strDCT2x2dn(p, o+80, o+144, o+96, o+160)
}

func strNormalizeDec(p []int32, o int, chroma bool) {
	if chroma {
		for i := 0; i < 256; i += 16 {
			p[o+i] += p[o+i]
		}
	}
}

func strPost4x4Stage1SplitAlternateGo(p []int32, p0, p1, iOffset int) {
	p2 := p0 + 72 - iOffset
	p3 := p1 + 64 - iOffset
	p0 += 12
	p1 += 4

	for k := 0; k < 4; k++ {
		strDCT2x2dn(p, p0+k, p2+k, p1+k, p3+k)
	}
	invOddOddPost(p, p3+0, p3+1, p3+2, p3+3)
	irotate1(p, p1+2, p1+3)
	irotate1(p, p1+0, p1+1)
	irotate1(p, p2+1, p2+3)
	irotate1(p, p2+0, p2+2)
	for k := 0; k < 4; k++ {
		strHSTdec1Alternate(p, p0+k, p3+k)
	}
	for k := 0; k < 4; k++ {
		strHSTdec(p, p0+k, p2+k, p1+k, p3+k)
	}
}

func strPost4x4Stage1Alternate(p []int32, o, iOffset int) {
	strPost4x4Stage1SplitAlternate(p, o, o+16, iOffset)
}

func strPost4x4Stage2SplitAlternate(p []int32, p0, p1 int) {
	strDCT2x2dn(p, p0-96, p0+96, p1-112, p1+80)
	strDCT2x2dn(p, p0-32, p0+32, p1-48, p1+16)
	strDCT2x2dn(p, p0-80, p0+112, p1-128, p1+64)
	strDCT2x2dn(p, p0-16, p0+48, p1-64, p1+0)

	invOddOddPost(p, p1+0, p1+64, p1+16, p1+80)

	irotate1(p, p0+48, p0+32)
	irotate1(p, p0+112, p0+96)
	irotate1(p, p1-64, p1-128)
	irotate1(p, p1-48, p1-112)

	strHSTdec1Alternate(p, p0-96, p1+80)
	strHSTdec1Alternate(p, p0-32, p1+16)
	strHSTdec1Alternate(p, p0-80, p1+64)
	strHSTdec1Alternate(p, p0-16, p1+0)

	strHSTdec(p, p0-96, p1-112, p0+96, p1+80)
	strHSTdec(p, p0-32, p1-48, p0+32, p1+16)
	strHSTdec(p, p0-80, p1-128, p0+112, p1+64)
	strHSTdec(p, p0-16, p1-64, p0+48, p1+0)
}

// invTransformMacroblockAlteredOperatorsHard is
// invTransformMacroblock_alteredOperators_hard, used for subversions 1 and 9.
func (sc *strCodec) invTransformMacroblockAlteredOperatorsHard() {
	ol := sc.wmiscp.olOverlap
	cf := sc.param.cfColorFormat
	left := sc.cColumn == 0
	right := sc.cColumn == sc.cmbWidth
	top := sc.cRow == 0
	bottom := sc.cRow == sc.cmbHeight
	topORbottom := top || bottom
	leftORright := left || right
	bottomORright := bottom || right
	leftAdjacentColumn := sc.cColumn == 1
	rightAdjacentColumn := sc.cColumn == sc.cmbWidth-1
	iChannels := sc.param.cNumChannels
	if cf == YUV420 || cf == YUV422 {
		iChannels = 1
	}
	w := &sc.wmiscp

	if w.bUseHardTileBoundaries {
		if sc.cColumn == 0 {
			sc.vertTileBoundary = false
			sc.tileY = 0
		}
		sc.oneMBLeftVertTB, sc.oneMBRightVertTB = false, false
		// jxrlib compares columns with uiTileY; kept for bit-exactness.
		if sc.tileY > 0 && sc.tileY <= w.cNumOfSliceMinus1H && uint32(sc.cColumn-1) == w.uiTileY[sc.tileY] {
			sc.oneMBRightVertTB = true
		}
		if sc.tileY < w.cNumOfSliceMinus1H && uint32(sc.cColumn) == w.uiTileY[sc.tileY+1] {
			sc.vertTileBoundary = true
			sc.tileY++
		} else {
			sc.vertTileBoundary = false
		}
		if sc.tileY < w.cNumOfSliceMinus1H && uint32(sc.cColumn+1) == w.uiTileY[sc.tileY+1] {
			sc.oneMBLeftVertTB = true
		}
		if sc.cRow == 0 {
			sc.horiTileBoundary = false
			sc.tileX = 0
		} else if sc.mbY != sc.cRow && sc.tileX < w.cNumOfSliceMinus1V && uint32(sc.cRow) == w.uiTileX[sc.tileX+1] {
			sc.horiTileBoundary = true
			sc.tileX++
		} else if sc.mbY != sc.cRow {
			sc.horiTileBoundary = false
		}
	} else {
		sc.vertTileBoundary = false
		sc.horiTileBoundary = false
		sc.oneMBLeftVertTB = false
		sc.oneMBRightVertTB = false
	}
	sc.mbX, sc.mbY = sc.cColumn, sc.cRow
	vtb := sc.vertTileBoundary
	htb := sc.horiTileBoundary
	oneL := sc.oneMBLeftVertTB
	oneR := sc.oneMBRightVertTB

	//================================================================
	// 400_Y, 444_YUV
	for i := 0; i < iChannels; i++ {
		p := sc.mbBuf[i]
		p0 := sc.p0(i)
		p1 := sc.p1(i)

		if !bottomORright {
			strIDCT4x4Stage2(p, p1)
			if sc.param.bScaledArith {
				strNormalizeDec(p, p1, i != 0)
			}
		}

		if ol == olTwo {
			if (top || htb) && (left || vtb) {
				strPost4Alternate(p, p1+0, p1+64, p1+0+16, p1+64+16)
			}
			if (top || htb) && (right || vtb) {
				strPost4Alternate(p, p1-128, p1-64, p1-128+16, p1-64+16)
			}
			if (bottom || htb) && (left || vtb) {
				strPost4Alternate(p, p0+32, p0+96, p0+32+16, p0+96+16)
			}
			if (bottom || htb) && (right || vtb) {
				strPost4Alternate(p, p0-96, p0-32, p0-96+16, p0-32+16)
			}
			if (leftORright || vtb) && (!topORbottom && !htb) {
				if left || vtb {
					j := 0
					strPost4Alternate(p, p0+j+32, p0+j+48, p1+j+0, p1+j+16)
					strPost4Alternate(p, p0+j+96, p0+j+112, p1+j+64, p1+j+80)
				}
				if right || vtb {
					j := -128
					strPost4Alternate(p, p0+j+32, p0+j+48, p1+j+0, p1+j+16)
					strPost4Alternate(p, p0+j+96, p0+j+112, p1+j+64, p1+j+80)
				}
			}
			if !leftORright {
				if (topORbottom || htb) && !vtb {
					if top || htb {
						q := p1
						strPost4Alternate(p, q-128, q-64, q+0, q+64)
						strPost4Alternate(p, q-112, q-48, q+16, q+80)
					}
					if bottom || htb {
						q := p0 + 32
						strPost4Alternate(p, q-128, q-64, q+0, q+64)
						strPost4Alternate(p, q-112, q-48, q+16, q+80)
					}
				}
				if !topORbottom && !htb && !vtb {
					strPost4x4Stage2SplitAlternate(p, p0, p1)
				}
			}
		}

		if !top {
			jEnd := 160
			if right {
				jEnd = 32
			}
			j := -96
			if left {
				j = 32
			}
			strIDCT4x4Stage1Pairs(p, p0+j, (jEnd-j)/64)
		}
		if !bottom {
			jEnd := 128
			if right {
				jEnd = 0
			}
			j := -128
			if left {
				j = 0
			}
			strIDCT4x4Stage1Pairs(p, p1+j, (jEnd-j)/64)
		}

		if ol != olNone {
			if leftORright || vtb {
				if (top || htb) && (left || vtb) {
					strPost4Alternate(p, p1+0, p1+1, p1+2, p1+3)
				}
				if (top || htb) && (right || vtb) {
					strPost4Alternate(p, p1-59, p1-60, p1-57, p1-58)
				}
				if (bottom || htb) && (left || vtb) {
					strPost4Alternate(p, p0+48+10, p0+48+11, p0+48+8, p0+48+9)
				}
				if (bottom || htb) && (right || vtb) {
					strPost4Alternate(p, p0-1, p0-2, p0-3, p0-4)
				}
				edge := func(j int) {
					if !top {
						q := p0 + 16 + j
						strPost4Alternate(p, q+0, q-2, q+6, q+8)
						strPost4Alternate(p, q+1, q-1, q+7, q+9)
						strPost4Alternate(p, q+16, q+14, q+22, q+24)
						strPost4Alternate(p, q+17, q+15, q+23, q+25)
					}
					if !bottom {
						q := p1 + j
						strPost4Alternate(p, q+0, q-2, q+6, q+8)
						strPost4Alternate(p, q+1, q-1, q+7, q+9)
					}
					if !topORbottom && !htb {
						strPost4Alternate(p, p0+48+j+0, p0+48+j-2, p1-10+j, p1-8+j)
						strPost4Alternate(p, p0+48+j+1, p0+48+j-1, p1-9+j, p1-7+j)
					}
				}
				if left || vtb {
					edge(0 + 10)
				}
				if right || vtb {
					edge(-64 + 14)
				}
			}

			jStart := -192
			if left {
				jStart = 0
			}
			jEnd := 64
			if right {
				jEnd = -64
			}
			if top || htb {
				for j := jStart; j < jEnd; j += 64 {
					if !vtb || j != -64 {
						q := p1 + j
						strPost4Alternate(p, q+5, q+4, q+64, q+65)
						strPost4Alternate(p, q+7, q+6, q+66, q+67)
						strPost4x4Stage1Alternate(p, p1+j, 0)
					}
				}
			}
			if bottom || htb {
				for j := jStart; j < jEnd; j += 64 {
					if !vtb || j != -64 {
						strPost4x4Stage1Alternate(p, p0+16+j, 0)
						strPost4x4Stage1Alternate(p, p0+32+j, 0)
						q := p0 + 48 + j
						strPost4Alternate(p, q+15, q+14, q+74, q+75)
						strPost4Alternate(p, q+13, q+12, q+72, q+73)
					}
				}
			}
			if !top && !bottom && !htb {
				for j := jStart; j < jEnd; j += 64 {
					if !vtb || j != -64 {
						strPost4x4Stage1Alternate(p, p0+16+j, 0)
						strPost4x4Stage1Alternate(p, p0+32+j, 0)
						strPost4x4Stage1SplitAlternate(p, p0+48+j, p1+j, 0)
						strPost4x4Stage1Alternate(p, p1+j, 0)
					}
				}
			}
		}
	}

	//================================================================
	// 420_UV
	if cf == YUV420 {
		for i := 0; i < 2; i++ {
			p := sc.mbBuf[1+i]
			p0 := sc.p0(1 + i)
			p1 := sc.p1(1 + i)

			if !bottomORright {
				if !sc.param.bScaledArith {
					strDCT2x2dn(p, p1, p1+32, p1+16, p1+48)
				} else {
					strDCT2x2dnDec(p, p1, p1+32, p1+16, p1+48)
				}
			}

			if ol == olTwo {
				if (leftAdjacentColumn || oneR) && (top || htb) {
					p[p1-64+0] -= p[p1-64+32]
				}
				if (rightAdjacentColumn || oneL) && (top || htb) {
					sc.iPredBefore[i][0] = p[p1+0]
				}
				if (right || vtb) && (top || htb) {
					p[p1-64+32] -= sc.iPredBefore[i][0]
				}
				if (leftAdjacentColumn || oneR) && (bottom || htb) {
					p[p0-64+16] -= p[p0-64+48]
				}
				if (rightAdjacentColumn || oneL) && (bottom || htb) {
					sc.iPredBefore[i][1] = p[p0+16]
				}
				if (right || vtb) && (bottom || htb) {
					p[p0-64+48] -= sc.iPredBefore[i][1]
				}

				if (leftORright || vtb) && !topORbottom && !htb {
					if left || vtb {
						strPost2Alternate(p, p0+0+16, p1+0)
					}
					if right || vtb {
						strPost2Alternate(p, p0-32+16, p1-32)
					}
				}
				if !leftORright {
					if (topORbottom || htb) && !vtb {
						if top || htb {
							strPost2Alternate(p, p1-32, p1)
						}
						if bottom || htb {
							strPost2Alternate(p, p0+16-32, p0+16)
						}
					} else if !topORbottom && !htb && !vtb {
						strPost2x2Alternate(p, p0-16, p0+16, p1-32, p1)
					}
				}

				if (leftAdjacentColumn || oneR) && (top || htb) {
					p[p1-64+0] += p[p1-64+32]
				}
				if (rightAdjacentColumn || oneL) && (top || htb) {
					sc.iPredAfter[i][0] = p[p1+0]
				}
				if (right || vtb) && (top || htb) {
					p[p1-64+32] += sc.iPredAfter[i][0]
				}
				if (leftAdjacentColumn || oneR) && (bottom || htb) {
					p[p0-64+16] += p[p0-64+48]
				}
				if (rightAdjacentColumn || oneL) && (bottom || htb) {
					sc.iPredAfter[i][1] = p[p0+16]
				}
				if (right || vtb) && (bottom || htb) {
					p[p0-64+48] += sc.iPredAfter[i][1]
				}
			}

			if !top {
				j := -16
				if left {
					j = 48
				} else if leftAdjacentColumn || oneR {
					j = -48
				}
				jEnd := 48
				if right || vtb {
					jEnd = 16
				}
				for ; j < jEnd; j += 32 {
					strIDCT4x4Stage1(p, p0+j)
				}
			}
			if !bottom {
				j := -32
				if left {
					j = 32
				} else if leftAdjacentColumn || oneR {
					j = -64
				}
				jEnd := 32
				if right || vtb {
					jEnd = 0
				}
				for ; j < jEnd; j += 32 {
					strIDCT4x4Stage1(p, p1+j)
				}
			}

			if ol != olNone {
				if (top || htb) && (leftAdjacentColumn || oneR) {
					strPost4Alternate(p, p1-64+0, p1-64+1, p1-64+2, p1-64+3)
				}
				if (top || htb) && (right || vtb) {
					strPost4Alternate(p, p1-27, p1-28, p1-25, p1-26)
				}
				if (bottom || htb) && (leftAdjacentColumn || oneR) {
					strPost4Alternate(p, p0-64+16+10, p0-64+16+11, p0-64+16+8, p0-64+16+9)
				}
				if (bottom || htb) && (right || vtb) {
					strPost4Alternate(p, p0-1, p0-2, p0-3, p0-4)
				}
				if !left && !top {
					if leftAdjacentColumn || oneR {
						if !bottom && !htb {
							strPost4Alternate(p, p0-64+26, p0-64+24, p1-64+0, p1-64+2)
							strPost4Alternate(p, p0-64+27, p0-64+25, p1-64+1, p1-64+3)
						}
						strPost4Alternate(p, p0-64+10, p0-64+8, p0-64+16, p0-64+18)
						strPost4Alternate(p, p0-64+11, p0-64+9, p0-64+17, p0-64+19)
					}
					if bottom || htb {
						q := p0 - 48
						strPost4Alternate(p, q+15, q+14, q+42, q+43)
						strPost4Alternate(p, q+13, q+12, q+40, q+41)
						if !right && !vtb {
							q = p0 - 16
							strPost4Alternate(p, q+15, q+14, q+42, q+43)
							strPost4Alternate(p, q+13, q+12, q+40, q+41)
						}
					} else {
						strPost4x4Stage1SplitAlternate(p, p0-48, p1-16-48, 32)
						if !right && !vtb {
							strPost4x4Stage1SplitAlternate(p, p0-16, p1-16-16, 32)
						}
					}
					if right || vtb {
						if !bottom && !htb {
							strPost4Alternate(p, p0-2, p0-4, p1-28, p1-26)
							strPost4Alternate(p, p0-1, p0-3, p1-27, p1-25)
						}
						strPost4Alternate(p, p0-18, p0-20, p0-12, p0-10)
						strPost4Alternate(p, p0-17, p0-19, p0-11, p0-9)
					} else {
						strPost4x4Stage1Alternate(p, p0-32, 32)
					}
					strPost4x4Stage1Alternate(p, p0-64, 32)
				}
				if top || htb {
					if !left {
						q := p1 - 64 + 4
						strPost4Alternate(p, q+1, q+0, q+28, q+29)
						strPost4Alternate(p, q+3, q+2, q+30, q+31)
					}
					if !left && !right && !vtb {
						q := p1 - 32 + 4
						strPost4Alternate(p, q+1, q+0, q+28, q+29)
						strPost4Alternate(p, q+3, q+2, q+30, q+31)
					}
				}
			}
		}
	}

	//================================================================
	// 422_UV
	if cf == YUV422 {
		for i := 0; i < 2; i++ {
			p := sc.mbBuf[1+i]
			p0 := sc.p0(1 + i)
			p1 := sc.p1(1 + i)

			if !bottomORright {
				p[p1+0] -= (p[p1+32] + 1) >> 1
				p[p1+32] += p[p1+0]
				if !sc.param.bScaledArith {
					strDCT2x2dn(p, p1+0, p1+64, p1+16, p1+80)
					strDCT2x2dn(p, p1+32, p1+96, p1+48, p1+112)
				} else {
					strDCT2x2dnDec(p, p1+0, p1+64, p1+16, p1+80)
					strDCT2x2dnDec(p, p1+32, p1+96, p1+48, p1+112)
				}
			}

			if ol == olTwo {
				if (leftAdjacentColumn || oneR) && (top || htb) {
					p[p1-128+0] -= p[p1-128+64]
				}
				if (rightAdjacentColumn || oneL) && (top || htb) {
					sc.iPredBefore[i][0] = p[p1+0]
				}
				if (right || vtb) && (top || htb) {
					p[p1-128+64] -= sc.iPredBefore[i][0]
				}
				if (leftAdjacentColumn || oneR) && (bottom || htb) {
					p[p0-128+48] -= p[p0-128+112]
				}
				if (rightAdjacentColumn || oneL) && (bottom || htb) {
					sc.iPredBefore[i][1] = p[p0+48]
				}
				if (right || vtb) && (bottom || htb) {
					p[p0-128+112] -= sc.iPredBefore[i][1]
				}

				if !bottom {
					if leftORright || vtb {
						if !top && !htb {
							if left || vtb {
								strPost2Alternate(p, p0+48+0, p1+0)
							}
							if right || vtb {
								strPost2Alternate(p, p0+48-64, p1-64)
							}
						}
						if left || vtb {
							strPost2Alternate(p, p1+16, p1+16+16)
						}
						if right || vtb {
							strPost2Alternate(p, p1-48, p1-48+16)
						}
					}
					if !leftORright && !vtb {
						if top || htb {
							strPost2Alternate(p, p1-64, p1)
						} else {
							strPost2x2Alternate(p, p0-16, p0+48, p1-64, p1)
						}
						strPost2x2Alternate(p, p1-48, p1+16, p1-32, p1+32)
					}
				}
				if (bottom || htb) && (!leftORright && !vtb) {
					strPost2Alternate(p, p0-16, p0+48)
				}

				if (leftAdjacentColumn || oneR) && (top || htb) {
					p[p1-128+0] += p[p1-128+64]
				}
				if (rightAdjacentColumn || oneL) && (top || htb) {
					sc.iPredAfter[i][0] = p[p1+0]
				}
				if (right || vtb) && (top || htb) {
					p[p1-128+64] += sc.iPredAfter[i][0]
				}
				if (leftAdjacentColumn || oneR) && (bottom || htb) {
					p[p0-128+48] += p[p0-128+112]
				}
				if (rightAdjacentColumn || oneL) && (bottom || htb) {
					sc.iPredAfter[i][1] = p[p0+48]
				}
				if (right || vtb) && (bottom || htb) {
					p[p0-128+112] += sc.iPredAfter[i][1]
				}
			}

			if !top {
				j := -16
				if left {
					j = 112
				} else if leftAdjacentColumn || oneR {
					j = -80
				}
				jEnd := 112
				if right || vtb {
					jEnd = 48
				}
				for ; j < jEnd; j += 64 {
					strIDCT4x4Stage1(p, p0+j)
				}
			}
			if !bottom {
				j := -64
				if left {
					j = 64
				} else if leftAdjacentColumn || oneR {
					j = -128
				}
				jEnd := 64
				if right || vtb {
					jEnd = 0
				}
				for ; j < jEnd; j += 64 {
					strIDCT4x4Stage1(p, p1+j+0)
					strIDCT4x4Stage1(p, p1+j+16)
					strIDCT4x4Stage1(p, p1+j+32)
				}
			}

			if ol != olNone {
				if (top || htb) && (leftAdjacentColumn || oneR) {
					strPost4Alternate(p, p1-128+0, p1-128+1, p1-128+2, p1-128+3)
				}
				if (top || htb) && (right || vtb) {
					strPost4Alternate(p, p1-59, p1-60, p1-57, p1-58)
				}
				if (bottom || htb) && (leftAdjacentColumn || oneR) {
					strPost4Alternate(p, p0-128+48+10, p0-128+48+11, p0-128+48+8, p0-128+48+9)
				}
				if (bottom || htb) && (right || vtb) {
					strPost4Alternate(p, p0-1, p0-2, p0-3, p0-4)
				}
				jStart := -128
				if left {
					jStart = 0
				}
				jEnd := 0
				if right || vtb {
					jEnd = -64
				}
				if !top {
					if leftAdjacentColumn || oneR {
						q := p0 + 32 + 10 - 128
						strPost4Alternate(p, q+0, q-2, q+6, q+8)
						strPost4Alternate(p, q+1, q-1, q+7, q+9)
					}
					if right || vtb {
						q := p0 - 32 + 14
						strPost4Alternate(p, q+0, q-2, q+6, q+8)
						strPost4Alternate(p, q+1, q-1, q+7, q+9)
					}
					for j := jStart; j < jEnd; j += 64 {
						strPost4x4Stage1Alternate(p, p0+j+32, 0)
					}
				}
				if !bottom {
					if leftAdjacentColumn || oneR {
						q := p1 + 0 + 10 - 128
						strPost4Alternate(p, q+0, q-2, q+6, q+8)
						strPost4Alternate(p, q+1, q-1, q+7, q+9)
						q += 16
						strPost4Alternate(p, q+0, q-2, q+6, q+8)
						strPost4Alternate(p, q+1, q-1, q+7, q+9)
					}
					if right || vtb {
						q := p1 - 64 + 14
						strPost4Alternate(p, q+0, q-2, q+6, q+8)
						strPost4Alternate(p, q+1, q-1, q+7, q+9)
						q += 16
						strPost4Alternate(p, q+0, q-2, q+6, q+8)
						strPost4Alternate(p, q+1, q-1, q+7, q+9)
					}
					for j := jStart; j < jEnd; j += 64 {
						strPost4x4Stage1Alternate(p, p1+j+0, 0)
						strPost4x4Stage1Alternate(p, p1+j+16, 0)
					}
				}
				if topORbottom || htb {
					if top || htb {
						q := p1 + 5
						for j := jStart; j < jEnd; j += 64 {
							strPost4Alternate(p, q+j+0, q+j-1, q+j+59, q+j+60)
							strPost4Alternate(p, q+j+2, q+j+1, q+j+61, q+j+62)
						}
					}
					if bottom || htb {
						q := p0 + 48 + 13
						for j := jStart; j < jEnd; j += 64 {
							strPost4Alternate(p, q+j+0, q+j-1, q+j+59, q+j+60)
							strPost4Alternate(p, q+j+2, q+j+1, q+j+61, q+j+62)
						}
					}
				} else {
					if leftAdjacentColumn || oneR {
						j := 0 + 0 - 128
						strPost4Alternate(p, p0+j+48+10+0, p0+j+48+10-2, p1+j+0, p1+j+2)
						strPost4Alternate(p, p0+j+48+10+1, p0+j+48+10-1, p1+j+1, p1+j+3)
					}
					if right || vtb {
						j := -64 + 4
						strPost4Alternate(p, p0+j+48+10+0, p0+j+48+10-2, p1+j+0, p1+j+2)
						strPost4Alternate(p, p0+j+48+10+1, p0+j+48+10-1, p1+j+1, p1+j+3)
					}
					for j := jStart; j < jEnd; j += 64 {
						strPost4x4Stage1SplitAlternate(p, p0+j+48, p1+j+0, 0)
					}
				}
			}
		}
	}
}
