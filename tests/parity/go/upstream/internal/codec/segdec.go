// Ported from jxrlib segdec.c, decode.c, image.c (BSD-2, see NOTICE).

package codec

import "math/bits"

const (
	huffmanDecodeRootBitsLog = 3
	huffmanDecodeRootBits    = 5
	maxTotal                 = 32767
	modelWeight              = 70
)

var grgiZigzagInv4x4Lowpass = [16]uint32{0, 1, 4, 5, 2, 8, 6, 9, 3, 12, 10, 7, 13, 11, 14, 15}
var grgiZigzagInv4x4H = [16]int{0, 1, 4, 5, 2, 8, 6, 9, 3, 12, 10, 7, 13, 11, 14, 15}
var grgiZigzagInv4x4V = [16]int{0, 4, 8, 5, 1, 12, 9, 6, 2, 13, 3, 15, 7, 10, 14, 11}

var gSignificantRunBin = [15]int{-1, -1, -1, -1, 2, 2, 2, 1, 1, 1, 1, 0, 0, 0, 0}
var gSignificantRunFixedLength = [15]uint32{0, 0, 1, 1, 3, 0, 0, 1, 1, 2, 0, 0, 0, 0, 1}

//================================================================
// image.c
//================================================================

var aWeight0 = [3]int32{240, 12, 1}
var aWeight1 = [3][maxChannels]int32{
	{0, 240, 120, 80, 60, 48, 40, 34, 30, 27, 24, 22, 20, 18, 17, 16},
	{0, 12, 6, 4, 3, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1},
	{0, 16, 8, 5, 4, 3, 3, 2, 2, 2, 2, 1, 1, 1, 1, 1},
}
var aWeight2 = [6]int32{120, 37, 2, 120, 18, 1}

func updateModelMB(cf int, iChannels int, lm *[2]int32, m *adaptiveModel) {
	lm[0] *= aWeight0[m.band-bandDC]
	switch cf {
	case YUV420:
		lm[1] *= aWeight2[m.band-bandDC]
	case YUV422:
		lm[1] *= aWeight2[3+m.band-bandDC]
	default:
		lm[1] *= aWeight1[m.band-bandDC][iChannels-1]
		if m.band == bandAC {
			lm[1] >>= 4
		}
	}
	for j := 0; j < 2; j++ {
		iLM := lm[j]
		iMS := m.flcState[j]
		iDelta := (iLM - modelWeight) >> 2
		if iDelta <= -8 {
			iDelta += 4
			if iDelta < -16 {
				iDelta = -16
			}
			iMS += iDelta
			if iMS < -8 {
				if m.flcBits[j] == 0 {
					iMS = -8
				} else {
					iMS = 0
					m.flcBits[j]--
				}
			}
		} else if iDelta >= 8 {
			iDelta -= 4
			if iDelta > 15 {
				iDelta = 15
			}
			iMS += iDelta
			if iMS > 8 {
				if m.flcBits[j] >= 15 {
					m.flcBits[j] = 15
					iMS = 8
				} else {
					iMS = 0
					m.flcBits[j]++
				}
			}
		}
		m.flcState[j] = iMS
		if cf == YOnly {
			break
		}
	}
}

func resetCodingContext(c *codingContext) {
	c.modelAC = adaptiveModel{band: bandAC}
	c.modelLP = adaptiveModel{band: bandLP, flcBits: [2]int32{4, 4}}
	c.modelDC = adaptiveModel{band: bandDC, flcBits: [2]int32{8, 8}}
	c.cbpCountMax, c.cbpCountZero = 1, 1
	c.cbpModel = cbpModel{count0: [2]int32{-4, -4}, count1: [2]int32{4, 4}}
}

func initZigzagScan(c *codingContext) {
	for i := 0; i < 16; i++ {
		c.scanLowpass[i].uScan = grgiZigzagInv4x4Lowpass[i]
		c.scanHoriz[i].uScan = uint32(dctIndex[0][grgiZigzagInv4x4H[i]])
		c.scanVert[i].uScan = uint32(dctIndex[0][grgiZigzagInv4x4V[i]])
	}
}

//================================================================
// decode.c
//================================================================

var aAlphabet = [numVLCTables]int{5, 4, 8, 7, 7, 12, 6, 6, 12, 6, 6, 7, 7, 12, 6, 6, 12, 6, 6, 7, 7}

func (sc *strCodec) allocateCodingContextDec(n int) error {
	if n > maxTiles || n < 1 {
		return ErrFormat
	}
	cbpSize := 9
	switch sc.param.cfColorFormat {
	case YOnly, NComponent, CMYK:
		cbpSize = 5
	}
	sc.contexts = reuse(sc.contexts, n)
	for i := range sc.contexts {
		c := &sc.contexts[i]
		c.ahCBPCY.nSymbols = cbpSize
		c.ahCBPCY1.nSymbols = 5
		for k := 0; k < numVLCTables; k++ {
			c.ahExpt[k].nSymbols = aAlphabet[k]
		}
		resetCodingContextDec(c)
	}
	return nil
}

func resetCodingContextDec(c *codingContext) {
	c.ahCBPCY.initialize = false
	c.ahCBPCY1.initialize = false
	for k := 0; k < numVLCTables; k++ {
		c.ahExpt[k].initialize = false
	}
	adaptLowpassDec(c)
	adaptHighpassDec(c)
	initZigzagScan(c)
	resetCodingContext(c)
}

func adaptLowpassDec(c *codingContext) {
	for k := 0; k < contextX+ctDC; k++ {
		c.ahExpt[k].adaptDiscriminant()
	}
}

func adaptHighpassDec(c *codingContext) {
	c.ahCBPCY.adaptDiscriminant()
	c.ahCBPCY1.adaptDiscriminant()
	for k := 0; k < contextX; k++ {
		c.ahExpt[k+contextX+ctDC].adaptDiscriminant()
	}
}

//================================================================
// segdec.c
//================================================================

func getHuff(t []int16, io *bitIO) int32 {
	sym := int32(t[io.peekBit16(huffmanDecodeRootBits)])
	if sym < 0 {
		io.flushBit16(huffmanDecodeRootBits)
	} else {
		io.flushBit16(uint32(sym & ((1 << huffmanDecodeRootBitsLog) - 1)))
	}
	huff := sym >> huffmanDecodeRootBitsLog
	if huff < 0 {
		huff = sym
		for {
			idx := int(huff) + 32768 + int(io.getBit16(1))
			if idx < 0 || idx >= len(t) {
				return 0
			}
			huff = int32(t[idx])
			if huff >= 0 {
				break
			}
		}
	}
	return huff
}

func getHuffShort(t []int16, io *bitIO) int32 {
	sym := int32(t[io.peekBit16(huffmanDecodeRootBits)])
	io.flushBit16(uint32(sym & ((1 << huffmanDecodeRootBitsLog) - 1)))
	return sym >> huffmanDecodeRootBitsLog
}

var cbpTab = [4]int32{6, 9, 10, 12}
var gFLC0 = [6]uint32{0, 2, 1, 2, 2, 0}
var gOff0 = [6]uint32{0, 4, 2, 8, 12, 1}
var gOut0 = [16]int32{0, 15, 3, 12, 1, 2, 4, 8, 5, 6, 9, 10, 7, 11, 13, 14}

func (sc *strCodec) decodeCBP(ctx *codingContext) {
	io := ctx.ioAC
	cf := sc.param.cfColorFormat
	iChannel := 1
	if cf == NComponent || cf == CMYK {
		iChannel = sc.param.cNumChannels
	}
	ahCBP := &ctx.ahCBPCY
	ahCBP1 := &ctx.ahCBPCY1
	ahEx1 := &ctx.ahExpt[1]

	for i := 0; i < iChannel; i++ {
		var cy, cu, cv int32
		n := getHuffShort(ahCBP1.hufDecTable[:], io)
		ahCBP1.disc += ahCBP1.delta[n]
		switch n {
		case 2:
			n = int32(io.getBit16(2))
			switch n {
			case 0:
				n = 3
			case 1:
				n = 5
			default:
				n = cbpTab[n*2+int32(io.getBool16())-4]
			}
		case 1:
			n = 1 << io.getBit16(2)
		case 3:
			n = 0xf ^ (1 << io.getBit16(2))
		case 4:
			n = 0xf
		}

		for iBlock := 0; iBlock < 4; iBlock++ {
			if n&(1<<iBlock) == 0 {
				continue
			}
			nb := getHuff(ahCBP.hufDecTable[:], io)
			val := uint32(nb) + 1
			ahCBP.disc += ahCBP.delta[nb]
			nb = 0
			if val >= 6 {
				if io.getBool16() != 0 {
					nb = 0x10
				} else if io.getBool16() != 0 {
					nb = 0x20
				} else {
					nb = 0x30
				}
				if val == 9 {
					if io.getBool16() != 0 {
					} else if io.getBool16() != 0 {
						val = 10
					} else {
						val = 11
					}
				}
				val -= 6
			}
			if val >= 6 {
				sc.err = ErrFormat
				return
			}
			code1 := gOff0[val]
			if gFLC0[val] != 0 {
				code1 += io.getBit16(gFLC0[val])
			}
			nb += gOut0[code1]

			switch cf {
			case YUV444:
				cy |= (nb & 0xf) << (iBlock * 4)
				for k := 0; k < 2; k++ {
					if (nb>>(k+4))&1 == 0 {
						continue
					}
					code := getHuffShort(ahEx1.hufDecTable[:], io)
					switch code {
					case 1:
						code = int32(io.getBit16(2))
						switch code {
						case 0:
							code = 3
						case 1:
							code = 5
						default:
							code = cbpTab[code*2+int32(io.getBool16())-4]
						}
					case 0:
						code = 1 << io.getBit16(2)
					case 2:
						code = 0xf ^ (1 << io.getBit16(2))
					case 3:
						code = 0xf
					}
					if k == 0 {
						cu |= code << (iBlock * 4)
					} else {
						cv |= code << (iBlock * 4)
					}
				}
			case YUV420:
				cy |= (nb & 0xf) << (iBlock * 4)
				cu |= ((nb >> 4) & 1) << iBlock
				cv |= ((nb >> 5) & 1) << iBlock
			case YUV422:
				cy |= (nb & 0xf) << (iBlock * 4)
				shift := [4]int{0, 1, 4, 5}
				for k := 0; k < 2; k++ {
					code := int32(5)
					if (nb>>(k+4))&1 != 0 {
						if io.getBool16() != 0 {
							code = 1
						} else if io.getBool16() != 0 {
							code = 4
						}
						code <<= shift[iBlock]
						if k == 0 {
							cu |= code
						} else {
							cv |= code
						}
					}
				}
			default:
				cy |= nb << (iBlock * 4)
			}
		}

		sc.mb.iDiffCBP[i] = cy
		if cf == YUV420 || cf == YUV444 || cf == YUV422 {
			sc.mb.iDiffCBP[1] = cu
			sc.mb.iDiffCBP[2] = cv
		}
	}
}

var runRemap = [15]int32{1, 2, 3, 5, 7, 1, 2, 3, 5, 7, 1, 2, 3, 4, 5}

func decodeSignificantRun(iMaxRun int, ah *adaptiveHuffman, io *bitIO) int32 {
	if iMaxRun < 5 {
		if iMaxRun == 1 {
			return 1
		} else if io.getBool16() != 0 {
			return 1
		} else if iMaxRun == 2 || io.getBool16() != 0 {
			return 2
		} else if iMaxRun == 3 || io.getBool16() != 0 {
			return 3
		}
		return 4
	}
	if iMaxRun >= len(gSignificantRunBin) {
		return 16 // corrupt stream; C reads past the table
	}
	bin := gSignificantRunBin[iMaxRun]
	sym := int32(ah.hufDecTable[io.peekBit16(huffmanDecodeRootBits)]) // getHuffShort, inlined by hand
	io.flushBit16(uint32(sym & ((1 << huffmanDecodeRootBitsLog) - 1)))
	idx := int(sym>>huffmanDecodeRootBitsLog) + bin*5
	// Branchless: flc is often zero, unpredictably.
	flc := gSignificantRunFixedLength[idx]
	if io.nb < flc {
		io.refill()
	}
	run := runRemap[idx] + int32(io.acc>>1>>((63-flc)&63))
	io.flushBit16(flc)
	return run
}

func decodeFirstIndex(ah *adaptiveHuffman, io *bitIO) int32 {
	idx := getHuff(ah.hufDecTable[:], io)
	ah.disc += ah.delta[idx]
	ah.disc1 += ah.delta1[idx]
	return idx
}

func decodeIndex(iLoc int, ah *adaptiveHuffman, io *bitIO) int32 {
	if iLoc < 15 {
		sym := int32(ah.hufDecTable[io.peekBit16(huffmanDecodeRootBits)]) // getHuffShort, inlined by hand
		io.flushBit16(uint32(sym & ((1 << huffmanDecodeRootBitsLog) - 1)))
		idx := sym >> huffmanDecodeRootBitsLog
		ah.disc += ah.delta[idx]
		ah.disc1 += ah.delta1[idx]
		return idx
	} else if iLoc == 15 {
		if io.getBool16() == 0 {
			return 0
		} else if io.getBool16() == 0 {
			return 2
		}
		return 1 + 2*int32(io.getBool16())
	}
	return int32(io.getBit16(1))
}

var absLevelRemap = [6]int32{2, 3, 4, 6, 10, 14}
var absLevelFixedLength = [6]uint32{0, 0, 1, 2, 2, 2}

func decodeSignificantAbsLevel(ah *adaptiveHuffman, io *bitIO) int32 {
	idx := uint32(getHuff(ah.hufDecTable[:], io))
	if idx > 6 {
		return 0
	}
	ah.disc += ah.delta[idx]
	if idx < 2 {
		return int32(idx) + 2
	} else if idx < 6 {
		return absLevelRemap[idx] + int32(io.getBit16(absLevelFixedLength[idx]))
	}
	fixed := io.getBit16(4) + 4
	if fixed == 19 {
		fixed += io.getBit16(2)
		if fixed == 22 {
			fixed += io.getBit16(3)
		}
	}
	level := int32(2 + (uint32(1) << fixed))
	level += int32(io.getBit32(fixed))
	return level
}

// decodeBlock returns the run/level pairs of one lowpass block.
func decodeBlock(chroma bool, coef *[32]int32, ah *[numVLCTables]adaptiveHuffman, ctxOff int, io *bitIO, iLocation int) int {
	b := 0
	if chroma {
		b = 3
	}
	ah1 := ah[ctxOff+b:]
	iNumNonzero := 1

	idx := decodeFirstIndex(&ah1[0], io)
	iSR := idx & 1
	iSRn := idx >> 2
	iCont := iSR & iSRn
	sign := io.getSign()
	if idx&2 != 0 {
		coef[1] = (decodeSignificantAbsLevel(&ah[6+ctxOff+int(iCont)], io) ^ sign) - sign
	} else {
		coef[1] = 1 | sign
	}
	coef[0] = 0
	if iSR == 0 {
		coef[0] = decodeSignificantRun(15-iLocation, &ah[0], io)
	}
	iLocation += int(coef[0]) + 1

	for iSRn != 0 {
		if iNumNonzero >= 16 {
			return 17
		}
		iSR = iSRn & 1
		coef[iNumNonzero*2] = 0
		if iSR == 0 {
			coef[iNumNonzero*2] = decodeSignificantRun(15-iLocation, &ah[0], io)
		}
		iLocation += int(coef[iNumNonzero*2]) + 1
		idx = decodeIndex(iLocation, &ah1[iCont+1], io)
		iSRn = idx >> 1
		iCont &= iSRn
		sign = io.getSign()
		if idx&1 != 0 {
			coef[iNumNonzero*2+1] = (decodeSignificantAbsLevel(&ah[6+ctxOff+int(iCont)], io) ^ sign) - sign
		} else {
			coef[iNumNonzero*2+1] = 1 | sign
		}
		iNumNonzero++
	}
	return iNumNonzero
}

// decodeBlockHighpassGo keeps the bit accumulator in locals; io is
// current only across calls and at return.
func decodeBlockHighpassGo(chroma bool, ah *[numVLCTables]adaptiveHuffman, io *bitIO, iQP int32, coef []int32, scan *[16]adaptiveScan) int {
	const ctxOff = ctDC + contextX
	b := 0
	if chroma {
		b = 3
	}
	ah1 := ah[ctxOff+b:]
	tRun := &ah[0].hufDecTable
	iLoc := uint32(1)
	iNumNonzero := 1

	acc, nb := io.acc, io.nb
	if nb < 24 { // index, sign, level peek and run: 19 bits
		acc, nb = io.fill(acc, nb)
	}
	var idx int32
	a := &ah1[0]
	if sym := int32(a.hufDecTable[acc>>59]); sym >= 0 { // decodeFirstIndex, short code
		acc <<= uint32(sym) & 7
		nb -= uint32(sym) & 7
		idx = sym >> huffmanDecodeRootBitsLog
		a.disc += a.delta[idx&15]
		a.disc1 += a.delta1[idx&15]
	} else {
		io.acc, io.nb = acc, nb
		idx = decodeFirstIndex(a, io)
		acc, nb = io.acc, io.nb
		if nb < 16 {
			acc, nb = io.fill(acc, nb)
		}
	}
	iSR := idx & 1
	iSRn := idx >> 2
	iCont := iSR & iSRn
	sign := int32(int64(acc) >> 63)
	acc <<= 1
	nb--
	level := (iQP ^ sign) - sign
	if idx&2 != 0 {
		a := &ah[6+ctxOff+int(iCont)]
		if sym := int32(a.hufDecTable[acc>>59]); uint32(sym) < 2<<huffmanDecodeRootBitsLog { // level 2 or 3
			acc <<= uint32(sym) & 7
			nb -= uint32(sym) & 7
			a.disc += a.delta[sym>>huffmanDecodeRootBitsLog&15]
			level *= sym>>huffmanDecodeRootBitsLog + 2
		} else {
			io.acc, io.nb = acc, nb
			level *= decodeSignificantAbsLevel(a, io)
			acc, nb = io.acc, io.nb
			if nb < 8 {
				acc, nb = io.fill(acc, nb)
			}
		}
	}
	if iSR == 0 { // decodeSignificantRun(14), bin 0
		sym := int32(tRun[acc>>59])
		acc <<= uint32(sym) & 7
		nb -= uint32(sym) & 7
		ri := int(sym >> huffmanDecodeRootBitsLog)
		flc := gSignificantRunFixedLength[ri]
		iLoc += uint32(runRemap[ri] + int32(acc>>1>>((63-flc)&63)))
		acc <<= flc & 63
		nb -= flc
	}
	iLoc &= 0xf
	coef[scan[iLoc].uScan] = level
	scan[iLoc].uTotal++
	if iLoc != 0 && scan[iLoc].uTotal > scan[iLoc-1].uTotal {
		scan[iLoc], scan[iLoc-1] = scan[iLoc-1], scan[iLoc]
	}
	iLoc = (iLoc + 1) & 0xf

	for iSRn != 0 {
		iSR = iSRn & 1
		if iNumNonzero > 16 {
			io.acc, io.nb = acc, nb
			return iNumNonzero
		}
		if nb < 24 { // run, index, sign and level peek: 19 bits
			acc, nb = io.fill(acc, nb)
		}
		if iSR == 0 { // decodeSignificantRun, inlined by hand
			m := 14 - int(iLoc) // iMaxRun - 1
			var run uint32
			if m >= 4 {
				if m >= 14 {
					io.acc, io.nb = acc, nb
					return 16
				}
				sym := int32(tRun[acc>>59])
				acc <<= uint32(sym) & 7
				nb -= uint32(sym) & 7
				ri := int(sym>>huffmanDecodeRootBitsLog) + gSignificantRunBin[m+1]*5
				flc := gSignificantRunFixedLength[ri]
				run = uint32(runRemap[ri] + int32(acc>>1>>((63-flc)&63)))
				acc <<= flc & 63
				nb -= flc
			} else {
				if m < 0 {
					m = 3 // iMaxRun 0 reads like 4, then overruns
				}
				k := min(bits.LeadingZeros64(acc), m)
				n := uint32(k)
				if k < m {
					n++
				}
				acc <<= n
				nb -= n
				run = uint32(k) + 1
			}
			iLoc += run
			if iLoc >= 16 {
				io.acc, io.nb = acc, nb
				return 16
			}
		}
		// decodeIndex, inlined by hand
		if iLoc < 14 {
			a := &ah1[iCont+1]
			sym := int32(a.hufDecTable[acc>>59])
			acc <<= uint32(sym) & 7
			nb -= uint32(sym) & 7
			idx = sym >> huffmanDecodeRootBitsLog
			a.disc += a.delta[idx&15]
			a.disc1 += a.delta1[idx&15]
		} else if iLoc == 14 {
			if int64(acc) >= 0 {
				idx = 0
				acc <<= 1
				nb--
			} else if int64(acc<<1) >= 0 {
				idx = 2
				acc <<= 2
				nb -= 2
			} else {
				idx = 1 + 2*int32(acc>>61&1)
				acc <<= 3
				nb -= 3
			}
		} else {
			idx = int32(acc >> 63)
			acc <<= 1
			nb--
		}
		iSRn = idx >> 1
		iCont &= iSRn
		sign = int32(int64(acc) >> 63)
		acc <<= 1
		nb--
		level = (iQP ^ sign) - sign
		if idx&1 != 0 {
			a := &ah[6+ctxOff+int(iCont)]
			if sym := int32(a.hufDecTable[acc>>59]); uint32(sym) < 2<<huffmanDecodeRootBitsLog { // level 2 or 3
				acc <<= uint32(sym) & 7
				nb -= uint32(sym) & 7
				a.disc += a.delta[sym>>huffmanDecodeRootBitsLog&15]
				level *= sym>>huffmanDecodeRootBitsLog + 2
			} else {
				io.acc, io.nb = acc, nb
				level *= decodeSignificantAbsLevel(a, io)
				acc, nb = io.acc, io.nb
			}
		}
		coef[scan[iLoc].uScan] = level
		scan[iLoc].uTotal++
		if iLoc != 0 && scan[iLoc].uTotal > scan[iLoc-1].uTotal {
			scan[iLoc], scan[iLoc-1] = scan[iLoc-1], scan[iLoc]
		}
		iLoc = (iLoc + 1) & 0xf
		iNumNonzero++
	}
	io.acc, io.nb = acc, nb
	return iNumNonzero
}

func decodeBlockAdaptive(noSkip, chroma bool, ah *[numVLCTables]adaptiveHuffman, io, ioFL *bitIO,
	coef []int32, scan *[16]adaptiveScan, iModelBits, iTrim, iQP int32, order *[16]int, skipFlexbits bool) int {
	iNumNonzero := 0
	iFlex := iModelBits - iTrim
	if iFlex < 0 || skipFlexbits {
		iFlex = 0
	}
	if noSkip {
		iQP1 := iQP << uint32(iModelBits)
		iNumNonzero = decodeBlockHighpass(chroma, ah, io, iQP1, coef, scan)
	}
	if iFlex != 0 {
		f := uint32(iFlex)
		q := iQP << uint32(iTrim)
		if iQP+iTrim == 1 {
			q = 1
		}
		if !flexbits(coef, order, ioFL, f, q) {
			flexbitsGo(coef, order, ioFL, f, q)
		}
	}
	return iNumNonzero
}

// flexbitsGo reads f flexbits per coefficient, 1 to 15.
func flexbitsGo(coef []int32, order *[16]int, ioFL *bitIO, f uint32, q int32) {
	// Branchless: flexbit signs are unpredictable.
	acc, nb := ioFL.acc, ioFL.nb
	for k := 1; k < 16; k++ {
		pk := &coef[order[k]]
		c := *pk
		if nb < f+1 {
			acc, nb = ioFL.fill(acc, nb)
		}
		w := uint32(acc >> ((63 - f) & 63))
		v := int32(w >> 1)
		z := int32((uint32(c)|uint32(-c))>>31) - 1 // -1 iff c == 0
		m := c>>31 | -int32(w&1)&z
		*pk = c + q*((v^m)-m)
		n := f + uint32(-v)>>31&uint32(z)
		acc <<= n & 63
		nb -= n
	}
	ioFL.acc, ioFL.nb = acc, nb
}

func (sc *strCodec) decodeCoeffs(ctx *codingContext, io, ioFL *bitIO) error {
	t := &sc.tiles[sc.cTileColumn]
	cf := sc.param.cfColorFormat
	iChannels := sc.param.cNumChannels
	iPlanes := iChannels
	if cf == YUV420 || cf == YUV422 {
		iPlanes = 1
	}
	iModelBits := ctx.modelAC.flcBits[0]
	var lm [2]int32
	pLM := 0
	order := &dctIndex[0]
	chroma := false
	scan := &ctx.scanHoriz
	if sc.mb.iOrientation == 1 {
		scan = &ctx.scanVert
	}
	iCBPCU := sc.mb.iCBP[1]
	iCBPCV := sc.mb.iCBP[2]
	iCBPCY := sc.mb.iCBP[0]
	iNBlocks := 4
	switch cf {
	case YUV420:
		iNBlocks = 6
		iCBPCY += (iCBPCU << 16) + (iCBPCV << 20)
	case YUV422:
		iNBlocks = 8
		iCBPCY += (iCBPCU << 16) + (iCBPCV << 24)
	}

	for i := 0; i < iPlanes; i++ {
		iIndex := 0
		for iBlock := 0; iBlock < iNBlocks; iBlock++ {
			qch := 0
			if iPlanes > 1 {
				qch = i
			} else if iBlock > 3 {
				if cf == YUV420 {
					qch = iBlock - 3
				} else {
					qch = iBlock/2 - 1
				}
			}
			iQP := t.qHP[qch][sc.mb.iQIndexHP].iQP

			for iSub := 0; iSub < 4; iSub, iIndex, iCBPCY = iSub+1, iIndex+1, iCBPCY>>1 {
				var coef []int32
				if iBlock >= 4 {
					if cf == YUV420 {
						coef = sc.mbBuf[iBlock-3][sc.p1(iBlock-3)+blkOffsetUV[iSub]:]
					} else {
						ch := 1 + (1 & (iBlock >> 1))
						coef = sc.mbBuf[ch][sc.p1(ch)+(iBlock&1)*32+blkOffsetUV422[iSub]:]
					}
				} else {
					coef = sc.mbBuf[i][sc.p1(i)+blkOffset[iIndex&0xf]:]
				}
				n := decodeBlockAdaptive(iCBPCY&1 != 0, chroma, &ctx.ahExpt, io, ioFL, coef, scan,
					iModelBits, ctx.trimFlexBits, iQP, order, sc.dp.bSkipFlexbits)
				if n > 16 {
					return ErrFormat
				}
				lm[pLM] += int32(n)
			}
			if iBlock == 3 {
				iModelBits = ctx.modelAC.flcBits[1]
				pLM = 1
				chroma = true
			}
		}
		iCBPCY = sc.mb.iCBP[(i+1)&0xf]
	}
	updateModelMB(cf, iChannels, &lm, &ctx.modelAC)
	return nil
}

func decodeQPIndex(io *bitIO, cBits uint8) uint8 {
	if io.getBit16(1) == 0 {
		return 0
	}
	return uint8(io.getBit16(uint32(cBits)) + 1)
}

var lpRemap = [7]int{4, 1, 2, 3, 5, 6, 7}

func (sc *strCodec) decodeMacroblockLowpass(ctx *codingContext) error {
	cf := sc.param.cfColorFormat
	iChannels := sc.param.cNumChannels
	iFullPlanes := iChannels
	if cf == YUV420 || cf == YUV422 {
		iFullPlanes = 2
	}
	scan := &ctx.scanLowpass
	io := ctx.ioLP
	iModelBits := ctx.modelLP.flcBits[0]
	var rl [32]int32
	var lm [2]int32
	pLM := 0
	var iCBP int32
	mb := &sc.mb

	if sc.wmiscp.bfBitstreamFormat != spatial && sc.tiles[sc.cTileColumn].cBitsLP > 0 {
		mb.iQIndexLP = decodeQPIndex(io, sc.tiles[sc.cTileColumn].cBitsLP)
		if mb.iQIndexLP >= sc.tiles[sc.cTileColumn].cNumQPLP { // jxrlib reads past the table
			return ErrFormat
		}
	}

	if sc.resetRGITotals {
		const iScale = 2
		iWeight := uint32(iScale * 16)
		scan[0].uTotal = maxTotal
		for k := 1; k < 16; k++ {
			scan[k].uTotal = iWeight
			iWeight -= iScale
		}
	}

	if cf == YUV420 || cf == YUV422 || cf == YUV444 {
		iCountM, iCountZ := ctx.cbpCountMax, ctx.cbpCountZero
		iMax := int32(iFullPlanes*4 - 5)
		if iCountZ <= 0 || iCountM < 0 {
			iCBP = 0
			if io.getBool16() != 0 {
				iCBP = 1
				k := int32(io.getBit16(uint32(iFullPlanes - 1)))
				if k != 0 {
					iCBP = k*2 + int32(io.getBit16(1))
				}
			}
			if iCountM < iCountZ {
				iCBP = iMax - iCBP
			}
		} else {
			iCBP = int32(io.getBit16(uint32(iFullPlanes)))
		}
		if iCBP == iMax {
			iCountM += 1 - 4
		} else {
			iCountM++
		}
		if iCBP == 0 {
			iCountZ += 1 - 4
		} else {
			iCountZ++
		}
		if iCountM < -8 {
			iCountM = -8
		} else if iCountM > 7 {
			iCountM = 7
		}
		ctx.cbpCountMax = iCountM
		if iCountZ < -8 {
			iCountZ = -8
		} else if iCountZ > 7 {
			iCountZ = 7
		}
		ctx.cbpCountZero = iCountZ
	} else {
		for ch := 0; ch < iChannels; ch++ {
			iCBP |= int32(io.getBit16(1)) << ch
		}
	}

	for ch := 0; ch < iFullPlanes; ch++ {
		coef := &mb.iBlockDC[ch]
		if iCBP&1 != 0 {
			loc := 1
			if cf == YUV420 && ch == 1 {
				loc += 9
			}
			if cf == YUV422 && ch == 1 {
				loc++
			}
			n := decodeBlock(ch > 0, &rl, &ctx.ahExpt, ctDC, io, loc)
			if n > 16 {
				return ErrFormat
			}
			lm[pLM] += int32(n)
			if (cf == YUV420 || cf == YUV422) && ch != 0 {
				var tmp [16]int32
				remap := lpRemap[:]
				count := 14
				if cf == YUV420 {
					remap = lpRemap[1:]
					count = 6
				}
				idx := 0
				for k := 0; k < n; k++ {
					idx += int(rl[k*2])
					tmp[idx&0xf] = rl[k*2+1]
					idx++
				}
				for k := 0; k < count; k++ {
					mb.iBlockDC[(k&1)+1][remap[k>>1]] = tmp[k]
				}
			} else {
				idx := 1
				for k := 0; k < n; k++ {
					idx += int(rl[k*2])
					if idx < 1 || idx > 15 {
						return ErrFormat
					}
					coef[scan[idx].uScan] = rl[k*2+1]
					scan[idx].uTotal++
					if scan[idx].uTotal > scan[idx-1].uTotal {
						scan[idx], scan[idx-1] = scan[idx-1], scan[idx]
					}
					idx++
				}
			}
		}

		if iModelBits != 0 {
			mbits := uint32(iModelBits)
			if (cf == YUV420 || cf == YUV422) && ch != 0 {
				kmax := 8
				if cf == YUV420 {
					kmax = 4
				}
				for k := 1; k < kmax; k++ {
					for c := 1; c <= 2; c++ {
						d := &mb.iBlockDC[c][k]
						if *d > 0 {
							*d <<= mbits
							*d += int32(io.getBit32(mbits))
						} else if *d < 0 {
							*d <<= mbits
							*d -= int32(io.getBit32(mbits))
						} else {
							*d = int32(io.getBit32(mbits))
							if *d != 0 && io.getBool16() != 0 {
								*d = -*d
							}
						}
					}
				}
			} else {
				for k := 1; k < 16; k++ {
					if coef[k] > 0 {
						coef[k] <<= mbits
						coef[k] += int32(io.getBit32(mbits))
					} else if coef[k] < 0 {
						coef[k] <<= mbits
						coef[k] -= int32(io.getBit32(mbits))
					} else {
						coef[k] = io.getBit16s(mbits)
					}
				}
			}
		}
		pLM = 1
		iModelBits = ctx.modelLP.flcBits[1]
		iCBP >>= 1
	}

	updateModelMB(cf, iChannels, &lm, &ctx.modelLP)
	if sc.resetContext {
		adaptLowpassDec(ctx)
	}
	return nil
}

func (sc *strCodec) decodeMacroblockDC(ctx *codingContext) error {
	t := &sc.tiles[sc.cTileColumn]
	mb := &sc.mb
	cf := sc.param.cfColorFormat
	iChannels := sc.param.cNumChannels
	io := ctx.ioDC
	var lm [2]int32
	pLM := 0
	iModelBits := ctx.modelDC.flcBits[0]

	for i := 0; i < iChannels; i++ {
		mb.iBlockDC[i] = [16]int32{}
	}
	mb.iQIndexLP, mb.iQIndexHP = 0, 0

	if sc.wmiscp.bfBitstreamFormat == spatial && sc.wmiscp.sbSubband != sbDCOnly {
		if t.cBitsLP > 0 {
			mb.iQIndexLP = decodeQPIndex(io, t.cBitsLP)
		}
		if sc.wmiscp.sbSubband != sbNoHighpass && t.cBitsHP > 0 {
			mb.iQIndexHP = decodeQPIndex(io, t.cBitsHP)
		}
	}
	if t.cBitsHP == 0 && t.cNumQPHP > 1 {
		mb.iQIndexHP = mb.iQIndexLP
	}
	if mb.iQIndexLP >= t.cNumQPLP || mb.iQIndexHP >= t.cNumQPHP {
		return ErrFormat
	}

	readDC := func(nonzero bool, ah *adaptiveHuffman) int32 {
		var q int32
		if nonzero {
			q = decodeSignificantAbsLevel(ah, io) - 1
			lm[pLM]++
		}
		if iModelBits != 0 {
			q = (q << uint32(iModelBits)) | int32(io.getBit16(uint32(iModelBits)))
		}
		if q != 0 && io.getBool16() != 0 {
			q = -q
		}
		return q
	}

	if cf == YOnly || cf == CMYK || cf == NComponent {
		for i := 0; i < iChannels; i++ {
			mb.iBlockDC[i][0] = readDC(io.getBool16() != 0, &ctx.ahExpt[3])
			pLM = 1
			iModelBits = ctx.modelDC.flcBits[1]
		}
	} else {
		idx := getHuff(ctx.ahExpt[2].hufDecTable[:], io)
		qy := idx >> 2
		qu := (idx >> 1) & 1
		qv := idx & 1
		mb.iBlockDC[0][0] = readDC(qy != 0, &ctx.ahExpt[3])
		pLM = 1
		iModelBits = ctx.modelDC.flcBits[1]
		mb.iBlockDC[1][0] = readDC(qu != 0, &ctx.ahExpt[4])
		mb.iBlockDC[2][0] = readDC(qv != 0, &ctx.ahExpt[4])
	}

	updateModelMB(cf, iChannels, &lm, &ctx.modelDC)

	if sc.wmiscp.sbSubband == sbDCOnly && sc.resetContext {
		for kk := 2; kk < 5; kk++ {
			ctx.ahExpt[kk].adaptDiscriminant()
		}
	}
	return nil
}

func (sc *strCodec) decodeMacroblockHighpass(ctx *codingContext) error {
	if sc.resetRGITotals {
		const iScale = 2
		iWeight := uint32(iScale * 16)
		ctx.scanHoriz[0].uTotal, ctx.scanVert[0].uTotal = maxTotal, maxTotal
		for k := 1; k < 16; k++ {
			ctx.scanHoriz[k].uTotal, ctx.scanVert[k].uTotal = iWeight, iWeight
			iWeight -= iScale
		}
	}
	t := &sc.tiles[sc.cTileColumn]
	if sc.wmiscp.bfBitstreamFormat != spatial && t.cBitsHP > 0 {
		sc.mb.iQIndexHP = decodeQPIndex(ctx.ioAC, t.cBitsHP)
		if sc.mb.iQIndexHP >= t.cNumQPHP {
			return ErrFormat
		}
	} else if t.cBitsHP == 0 && t.cNumQPHP > 1 {
		sc.mb.iQIndexHP = sc.mb.iQIndexLP
	}

	sc.decodeCBP(ctx)
	if sc.err != nil {
		return sc.err
	}
	sc.predCBPDec(ctx)
	if err := sc.decodeCoeffs(ctx, ctx.ioAC, ctx.ioFL); err != nil {
		return err
	}
	if sc.resetContext {
		adaptHighpassDec(ctx)
	}
	return nil
}
