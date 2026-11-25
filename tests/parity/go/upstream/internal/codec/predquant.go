// Ported from jxrlib strPredQuant.c, strPredQuantDec.c (BSD-2, see NOTICE).

package codec

const orientWeight = 4

var qpRecipTable = [32][2]int64{
	{0x0, 0}, {0x0, 0}, {0x0, 1}, {0xaaaaaaab, 1},
	{0x0, 2}, {0xcccccccd, 2}, {0xaaaaaaab, 2}, {0x92492493, 2},
	{0x0, 3}, {0xe38e38e4, 3}, {0xcccccccd, 3}, {0xba2e8ba3, 3},
	{0xaaaaaaab, 3}, {0x9d89d89e, 3}, {0x92492493, 3}, {0x88888889, 3},
	{0x0, 4}, {0xf0f0f0f1, 4}, {0xe38e38e4, 4}, {0xd79435e6, 4},
	{0xcccccccd, 4}, {0xc30c30c4, 4}, {0xba2e8ba3, 4}, {0xb21642c9, 4},
	{0xaaaaaaab, 4}, {0xa3d70a3e, 4}, {0x9d89d89e, 4}, {0x97b425ee, 4},
	{0x92492493, 4}, {0x8d3dcb09, 4}, {0x88888889, 4}, {0x84210843, 4},
}

func remapQP(q *quantizer, iShift int32, bScaledArith bool) {
	idx := int32(q.iIndex)
	if idx == 0 {
		q.iQP, q.iMan, q.iExp, q.iOffset = 1, 0, 0, 0
		return
	}
	var man, exp int32
	if !bScaledArith {
		const ciShift = -qpFracBits
		if idx < 32 {
			man, exp = (idx+3)>>2, ciShift+2
		} else if idx < 48 {
			man, exp = (16+(idx&0xf)+1)>>1, ((idx>>4)-1)+1+ciShift
		} else {
			man, exp = 16+(idx&0xf), ((idx>>4)-1)+ciShift
		}
	} else {
		if idx < 16 {
			man, exp = idx, iShift
		} else {
			man, exp = 16+(idx&0xf), ((idx>>4)-1)+iShift
		}
	}
	q.iQP = man << uint32(exp)
	q.iMan = int32(qpRecipTable[man][0])
	q.iExp = int32(qpRecipTable[man][1]) + exp
	q.iOffset = (q.iQP*3 + 1) >> 3
}

func (sc *strCodec) allocatePredInfo() {
	for i := 0; i < sc.param.cNumChannels; i++ {
		sc.predInfo[i] = reuse(sc.predInfo[i], sc.cmbWidth)
		sc.predInfoPrev[i] = reuse(sc.predInfoPrev[i], sc.cmbWidth)
	}
}

func abs32(x int32) int32 {
	if x < 0 {
		return -x
	}
	return x
}

func getACPredMode(mb *mbInfo, cf int) int32 {
	c := &mb.iBlockDC[0]
	strH := abs32(c[1]) + abs32(c[2]) + abs32(c[3])
	strV := abs32(c[4]) + abs32(c[8]) + abs32(c[12])
	if cf != YOnly && cf != NComponent {
		u, v := &mb.iBlockDC[1], &mb.iBlockDC[2]
		strH += abs32(u[1]) + abs32(v[1])
		switch cf {
		case YUV420:
			strV += abs32(u[2]) + abs32(v[2])
		case YUV422:
			strV += abs32(u[2]) + abs32(v[2]) + abs32(u[6]) + abs32(v[6])
			strH += abs32(u[5]) + abs32(v[5])
		default:
			strV += abs32(u[4]) + abs32(v[4])
		}
	}
	if strH*orientWeight < strV {
		return 1
	}
	if strV*orientWeight < strH {
		return 0
	}
	return 2
}

func (sc *strCodec) getDCACPredMode(mbX int) int {
	var iDCMode int
	iADMode := 2
	if sc.ctxLeft && sc.ctxTop {
		iDCMode = 3
	} else if sc.ctxLeft {
		iDCMode = 1
	} else if sc.ctxTop {
		iDCMode = 0
	} else {
		cf := sc.param.cfColorFormat
		iL := sc.predInfo[0][mbX-1].iDC
		iT := sc.predInfoPrev[0][mbX].iDC
		iTL := sc.predInfoPrev[0][mbX-1].iDC
		var strH, strV int32
		if cf == YOnly || cf == NComponent {
			strH = abs32(iTL - iL)
			strV = abs32(iTL - iT)
		} else {
			tu, lu, tlu := &sc.predInfoPrev[1][mbX], &sc.predInfo[1][mbX-1], &sc.predInfoPrev[1][mbX-1]
			tv, lv, tlv := &sc.predInfoPrev[2][mbX], &sc.predInfo[2][mbX-1], &sc.predInfoPrev[2][mbX-1]
			scale := int32(2)
			switch cf {
			case YUV420:
				scale = 8
			case YUV422:
				scale = 4
			}
			strH = abs32(iTL-iL)*scale + abs32(tlu.iDC-lu.iDC) + abs32(tlv.iDC-lv.iDC)
			strV = abs32(iTL-iT)*scale + abs32(tlu.iDC-tu.iDC) + abs32(tlv.iDC-tv.iDC)
		}
		if strH*orientWeight < strV {
			iDCMode = 1
		} else if strV*orientWeight < strH {
			iDCMode = 0
		} else {
			iDCMode = 2
		}
	}
	if iDCMode == 1 && int32(sc.mb.iQIndexLP) == sc.predInfoPrev[0][mbX].iQPIndex {
		iADMode = 1
	}
	if iDCMode == 0 && int32(sc.mb.iQIndexLP) == sc.predInfo[0][mbX-1].iQPIndex {
		iADMode = 0
	}
	return iDCMode + (iADMode << 2)
}

func (sc *strCodec) updatePredInfo(mbX int, cf int) {
	iChannels := sc.param.cNumChannels
	if cf == YUV420 || cf == YUV422 {
		iChannels = 1
	}
	for i := 0; i < iChannels; i++ {
		pi := &sc.predInfo[i][mbX]
		p := &sc.mb.iBlockDC[i]
		pi.iDC = p[0]
		pi.iQPIndex = int32(sc.mb.iQIndexLP)
		pi.iAD[0], pi.iAD[1], pi.iAD[2] = p[1], p[2], p[3]
		pi.iAD[3], pi.iAD[4], pi.iAD[5] = p[4], p[8], p[12]
	}
	switch cf {
	case YUV420:
		for i := 1; i < 3; i++ {
			pi := &sc.predInfo[i][mbX]
			p := &sc.mb.iBlockDC[i]
			pi.iDC = p[0]
			pi.iQPIndex = int32(sc.mb.iQIndexLP)
			pi.iAD[0], pi.iAD[1] = p[1], p[2]
		}
	case YUV422:
		for i := 1; i < 3; i++ {
			pi := &sc.predInfo[i][mbX]
			pi.iQPIndex = int32(sc.mb.iQIndexLP)
			p := &sc.mb.iBlockDC[i]
			pi.iDC = p[0]
			pi.iAD[0], pi.iAD[1] = p[1], p[2]
			pi.iAD[2], pi.iAD[3] = p[5], p[6]
			pi.iAD[4] = p[4]
		}
	}
}

func (sc *strCodec) dequantizeMacroblock() {
	cf := sc.param.cfColorFormat
	mb := &sc.mb
	t := &sc.tiles[sc.cTileColumn]
	for i := 0; i < sc.param.cNumChannels; i++ {
		p := sc.mbBuf[i][sc.p1(i):]
		org := &mb.iBlockDC[i]
		p[0] = org[0] * t.qDC[i][0].iQP
		if sc.wmiscp.sbSubband == sbDCOnly {
			continue
		}
		qp := t.qLP[i][mb.iQIndexLP].iQP
		if i == 0 || (cf != YUV422 && cf != YUV420) {
			for k := 1; k < 16; k++ {
				p[dctIndex[2][k]] = org[k] * qp
			}
		} else if cf == YUV422 {
			p[64] = org[1] * qp
			p[16] = org[2] * qp
			p[80] = org[3] * qp
			p[32] = org[4] * qp
			p[96] = org[5] * qp
			p[48] = org[6] * qp
			p[112] = org[7] * qp
		} else {
			p[32] = org[1] * qp
			p[16] = org[2] * qp
			p[48] = org[3] * qp
		}
	}
}

func (sc *strCodec) predDCACDec() {
	cf := sc.param.cfColorFormat
	iChannels := sc.param.cNumChannels
	if cf == YUV420 || cf == YUV422 {
		iChannels = 1
	}
	mb := &sc.mb
	mbX := sc.cColumn
	mode := sc.getDCACPredMode(mbX)
	dcMode := mode & 3
	adMode := mode & 0xc

	for ii := 0; ii < iChannels; ii++ {
		org := &mb.iBlockDC[ii]
		switch dcMode {
		case 1:
			org[0] += sc.predInfoPrev[ii][mbX].iDC
		case 0:
			org[0] += sc.predInfo[ii][mbX-1].iDC
		case 2:
			org[0] += (sc.predInfo[ii][mbX-1].iDC + sc.predInfoPrev[ii][mbX].iDC) >> 1
		}
		switch adMode {
		case 4:
			ref := &sc.predInfoPrev[ii][mbX].iAD
			org[4] += ref[3]
			org[8] += ref[4]
			org[12] += ref[5]
		case 0:
			ref := &sc.predInfo[ii][mbX-1].iAD
			org[1] += ref[0]
			org[2] += ref[1]
			org[3] += ref[2]
		}
	}

	switch cf {
	case YUV420:
		for ii := 1; ii < 3; ii++ {
			org := &mb.iBlockDC[ii]
			switch dcMode {
			case 1:
				org[0] += sc.predInfoPrev[ii][mbX].iDC
			case 0:
				org[0] += sc.predInfo[ii][mbX-1].iDC
			case 2:
				org[0] += (sc.predInfo[ii][mbX-1].iDC + sc.predInfoPrev[ii][mbX].iDC + 1) >> 1
			}
			switch adMode {
			case 4:
				org[2] += sc.predInfoPrev[ii][mbX].iAD[1]
			case 0:
				org[1] += sc.predInfo[ii][mbX-1].iAD[0]
			}
		}
	case YUV422:
		for ii := 1; ii < 3; ii++ {
			org := &mb.iBlockDC[ii]
			switch dcMode {
			case 1:
				org[0] += sc.predInfoPrev[ii][mbX].iDC
			case 0:
				org[0] += sc.predInfo[ii][mbX-1].iDC
			case 2:
				org[0] += (sc.predInfo[ii][mbX-1].iDC + sc.predInfoPrev[ii][mbX].iDC + 1) >> 1
			}
			if adMode == 4 {
				org[4] += sc.predInfoPrev[ii][mbX].iAD[4]
				org[2] += sc.predInfoPrev[ii][mbX].iAD[3]
				org[6] += org[2]
			} else if adMode == 0 {
				org[4] += sc.predInfo[ii][mbX-1].iAD[4]
				org[1] += sc.predInfo[ii][mbX-1].iAD[0]
				org[5] += sc.predInfo[ii][mbX-1].iAD[2]
			} else if dcMode == 1 {
				org[6] += org[2]
			}
		}
	}

	mb.iOrientation = 2 - getACPredMode(mb, cf)
}

var predTopBlkIdx = [12]int{1, 2, 3, 5, 6, 7, 9, 10, 11, 13, 14, 15}

func (sc *strCodec) predACDec() {
	cf := sc.param.cfColorFormat
	iChannels := sc.param.cNumChannels
	if cf == YUV420 || cf == YUV422 {
		iChannels = 1
	}
	mode := 2 - sc.mb.iOrientation

	for i := 0; i < iChannels; i++ {
		src := sc.mbBuf[i][sc.p1(i):]
		switch mode {
		case 1:
			for _, b := range predTopBlkIdx {
				o := 16 * b
				r := o - 16
				src[o+2] += src[r+2]
				src[o+10] += src[r+10]
				src[o+9] += src[r+9]
			}
		case 0:
			for o := 64; o < 256; o += 16 {
				r := o - 64
				src[o+1] += src[r+1]
				src[o+5] += src[r+5]
				src[o+6] += src[r+6]
			}
		}
	}

	switch cf {
	case YUV420:
		for i := 1; i < 3; i++ {
			src := sc.mbBuf[i][sc.p1(i):]
			switch mode {
			case 1:
				for j := 1; j <= 3; j += 2 {
					o := 16 * j
					r := o - 16
					src[o+2] += src[r+2]
					src[o+10] += src[r+10]
					src[o+9] += src[r+9]
				}
			case 0:
				for j := 2; j <= 3; j++ {
					o := 16 * j
					r := o - 32
					src[o+1] += src[r+1]
					src[o+5] += src[r+5]
					src[o+6] += src[r+6]
				}
			}
		}
	case YUV422:
		for i := 1; i < 3; i++ {
			src := sc.mbBuf[i][sc.p1(i):]
			switch mode {
			case 1:
				for j := 2; j < 8; j++ {
					o := blkOffsetUV422[j]
					r := o - 16
					src[o+10] += src[r+10]
					src[o+2] += src[r+2]
					src[o+9] += src[r+9]
				}
			case 0:
				for j := 1; j < 8; j += 2 {
					o := blkOffsetUV422[j]
					r := o - 64
					src[o+1] += src[r+1]
					src[o+5] += src[r+5]
					src[o+6] += src[r+6]
				}
			}
		}
	}
}

func numOnes(i int32) int32 {
	var g = [16]int32{0, 1, 1, 2, 1, 2, 2, 3, 1, 2, 2, 3, 2, 3, 3, 4}
	var r int32
	i &= 0xffff
	for i != 0 {
		r += g[i&0xf]
		i >>= 4
	}
	return r
}

func saturate32(x *int32) {
	if uint32(*x+16) >= 32 {
		if *x < 0 {
			*x = -16
		} else {
			*x = 15
		}
	}
}

func updateCBPState(m *cbpModel, c1 int, iNOrig int32) {
	const iNDiff = 3
	m.count0[c1] += iNOrig - iNDiff
	saturate32(&m.count0[c1])
	m.count1[c1] += 16 - iNOrig - iNDiff
	saturate32(&m.count1[c1])
	if m.count0[c1] < 0 {
		if m.count0[c1] < m.count1[c1] {
			m.state[c1] = 1
		} else {
			m.state[c1] = 2
		}
	} else if m.count1[c1] < 0 {
		m.state[c1] = 2
	} else {
		m.state[c1] = 0
	}
}

func (sc *strCodec) predCBPCDec(iCBP int32, mbX, c int, m *cbpModel) int32 {
	c1 := 0
	if c != 0 {
		c1 = 1
	}
	switch m.state[c1] {
	case 0:
		if sc.ctxLeft {
			if sc.ctxTop {
				iCBP ^= 1
			} else {
				iCBP ^= (sc.predInfoPrev[c][mbX].iCBP >> 10) & 1
			}
		} else {
			iCBP ^= (sc.predInfo[c][mbX-1].iCBP >> 5) & 1
		}
		iCBP ^= 0x02 & (iCBP << 1)
		iCBP ^= 0x10 & (iCBP << 3)
		iCBP ^= 0x20 & (iCBP << 1)
		iCBP ^= (iCBP & 0x33) << 2
		iCBP ^= (iCBP & 0xcc) << 6
		iCBP ^= (iCBP & 0x3300) << 2
	case 2:
		iCBP ^= 0xffff
	}
	updateCBPState(m, c1, numOnes(iCBP))
	return iCBP
}

func (sc *strCodec) predCBPC420Dec(iCBP int32, mbX, c int, m *cbpModel) int32 {
	switch m.state[1] {
	case 0:
		if sc.ctxLeft {
			if sc.ctxTop {
				iCBP ^= 1
			} else {
				iCBP ^= (sc.predInfoPrev[c][mbX].iCBP >> 2) & 1
			}
		} else {
			iCBP ^= (sc.predInfo[c][mbX-1].iCBP >> 1) & 1
		}
		iCBP ^= 0x02 & (iCBP << 1)
		iCBP ^= (iCBP & 0x3) << 2
	case 2:
		iCBP ^= 0xf
	}
	updateCBPState(m, 1, numOnes(iCBP)*4)
	return iCBP
}

func (sc *strCodec) predCBPC422Dec(iCBP int32, mbX, c int, m *cbpModel) int32 {
	switch m.state[1] {
	case 0:
		if sc.ctxLeft {
			if sc.ctxTop {
				iCBP ^= 1
			} else {
				iCBP ^= (sc.predInfoPrev[c][mbX].iCBP >> 6) & 1
			}
		} else {
			iCBP ^= (sc.predInfo[c][mbX-1].iCBP >> 1) & 1
		}
		iCBP ^= (iCBP & 0x1) << 1
		iCBP ^= (iCBP & 0x3) << 2
		iCBP ^= (iCBP & 0xc) << 2
		iCBP ^= (iCBP & 0x30) << 2
	case 2:
		iCBP ^= 0xff
	}
	updateCBPState(m, 1, numOnes(iCBP)*2)
	return iCBP
}

func (sc *strCodec) predCBPDec(ctx *codingContext) {
	cf := sc.param.cfColorFormat
	iChannels := sc.param.cNumChannels
	if cf == YUV420 || cf == YUV422 {
		iChannels = 1
	}
	mbX := sc.cColumn
	mb := &sc.mb
	for i := 0; i < iChannels; i++ {
		mb.iCBP[i] = sc.predCBPCDec(mb.iDiffCBP[i], mbX, i, &ctx.cbpModel)
		sc.predInfo[i][mbX].iCBP = mb.iCBP[i]
	}
	switch cf {
	case YUV422:
		for c := 1; c < 3; c++ {
			mb.iCBP[c] = sc.predCBPC422Dec(mb.iDiffCBP[c], mbX, c, &ctx.cbpModel)
			sc.predInfo[c][mbX].iCBP = mb.iCBP[c]
		}
	case YUV420:
		for c := 1; c < 3; c++ {
			mb.iCBP[c] = sc.predCBPC420Dec(mb.iDiffCBP[c], mbX, c, &ctx.cbpModel)
			sc.predInfo[c][mbX].iCBP = mb.iCBP[c]
		}
	}
}
