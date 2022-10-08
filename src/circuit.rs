#![allow(clippy::borrow_deref_ref)]
use crate::error::Error;
use elektron_ngspice::{Callbacks, ComplexSlice, NgSpice};
use lazy_static::lazy_static;
use regex::Regex;
use std::{
    collections::HashMap,
    fs::{self, File},
    io::Write,
};

lazy_static! {
    pub static ref RE_SUBCKT: regex::Regex =
        Regex::new(r"(?i:\.SUBCKT) ([a-zA-Z0-9]*) .*").unwrap();
    pub static ref RE_MODEL: regex::Regex = Regex::new(r"(?i:\.model) ([a-zA-Z0-9]*) .*").unwrap();
    pub static ref RE_INCLUDE: regex::Regex = Regex::new(r"(?i:\.include) (.*)").unwrap();
}

pub struct Cb {
    strs: Vec<String>,
    status: i32, 
    unload: bool,
    quit: bool,
}

impl Cb {
    pub fn new() -> Self {
        Self {
            strs: Vec::new(),
            status: 0,
            unload: false,
            quit: false,
        }
    }
}

impl Callbacks for Cb {
    fn send_char(&mut self, s: &str) {
        if std::env::var("ELEKTRON_DEBUG").is_ok() {
            println!("{}", s);
        }
        self.strs.push(s.to_string())
    }
    fn controlled_exit(&mut self, status: i32, unload: bool, quit: bool) {
        self.status = status;
        self.unload = unload;
        self.quit = quit;
    }
}
#[derive(Debug, Clone, PartialEq)]
enum CircuitItem {
    R(String, String, String, String),
    C(String, String, String, String),
    D(String, String, String, String),
    Q(String, String, String, String, String),
    X(String, Vec<String>, String),
    V(String, String, String, String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Circuit {
    name: String,
    pathlist: Vec<String>,
    items: Vec<CircuitItem>,
    subcircuits: HashMap<String, (Vec<String>, Circuit)>,
}

impl Circuit {
    pub fn new(name: String, pathlist: Vec<String>) -> Self {
        Self {
            name,
            pathlist,
            items: Vec::new(),
            subcircuits: HashMap::new(),
        }
    }

    pub fn resistor(&mut self, reference: String, n0: String, n1: String, value: String) {
        self.items.push(CircuitItem::R(reference, n0, n1, value));
    }

    pub fn capacitor(&mut self, reference: String, n0: String, n1: String, value: String) {
        self.items.push(CircuitItem::C(reference, n0, n1, value));
    }

    pub fn diode(&mut self, reference: String, n0: String, n1: String, value: String) {
        self.items.push(CircuitItem::D(reference, n0, n1, value));
    }

    pub fn bjt(&mut self, reference: String, n0: String, n1: String, n2: String, value: String) {
        self.items
            .push(CircuitItem::Q(reference, n0, n1, n2, value));
    }

    pub fn circuit(
        &mut self,
        reference: String,
        n: Vec<String>,
        value: String,
    ) -> Result<(), Error> {
        //TODO self.get_includes(&value)?;
        self.items.push(CircuitItem::X(reference, n, value));
        Ok(())
    }
    pub fn subcircuit(
        &mut self,
        name: String,
        n: Vec<String>,
        circuit: Circuit,
    ) -> Result<(), Error> {
        self.subcircuits.insert(name, (n, circuit));
        Ok(())
    }
    pub fn voltage(&mut self, reference: String, n1: String, n2: String, value: String) {
        self.items.push(CircuitItem::V(reference, n1, n2, value));
    }
    pub fn save(&self, filename: Option<String>) -> Result<(), Error> {
        let mut out: Box<dyn Write> = if let Some(filename) = filename {
            Box::new(File::create(filename).unwrap())
        } else {
            Box::new(std::io::stdout())
        };
        for s in self.to_str(true).unwrap() {
            writeln!(out, "{}", s)?;
        }
        out.flush()?;
        Ok(())
    }
    pub fn set_value(&mut self, reference: &str, value: &str) -> Result<(), Error> {
        for item in &mut self.items.iter_mut() {
            match item {
                CircuitItem::R(r, _, _, ref mut v) => {
                    if reference == r {
                        *v = value.to_string();
                        return Ok(());
                    }
                }
                CircuitItem::C(r, _, _, ref mut v) => {
                    if reference == r {
                        *v = value.to_string();
                        return Ok(());
                    }
                }
                CircuitItem::D(r, _, _, ref mut v) => {
                    if reference == r {
                        *v = value.to_string();
                        return Ok(());
                    }
                }
                CircuitItem::Q(_, _, _, _, _) => {}
                CircuitItem::X(_, _, _) => {}
                CircuitItem::V(r, _, _, ref mut v) => {
                    if reference == r {
                        *v = value.to_string();
                        return Ok(());
                    }
                }
            }
        }
        Err(Error::UnknownCircuitElement(reference.to_string()))
    }
}

impl Circuit {
    fn get_includes(&self, key: String) -> Result<HashMap<String, String>, Error> {
        let mut result: HashMap<String, String> = HashMap::new();
        for path in &self.pathlist {
            for entry in fs::read_dir(path).unwrap() {
                let dir = entry.unwrap();
                if dir.path().is_file() {
                    let content = fs::read_to_string(dir.path())?;
                    for cap in RE_SUBCKT.captures_iter(&content) {
                        let text1 = cap.get(1).map_or("", |m| m.as_str());
                        if text1 == key {
                            result.insert(key, dir.path().to_str().unwrap().to_string());
                            if let Some(caps) = RE_INCLUDE.captures(&content) {
                                for cap in caps.iter().skip(1) {
                                    let text1 = cap.map_or("", |m| m.as_str());
                                    if !text1.contains('/') {
                                        //when there is no slash i could be
                                        //a relative path.
                                        let mut parent = dir
                                            .path()
                                            .parent()
                                            .unwrap()
                                            .to_str()
                                            .unwrap()
                                            .to_string();
                                        parent += "/";
                                        parent += text1;
                                        result.insert(text1.to_string(), parent.to_string());
                                    } else {
                                        result.insert(text1.to_string(), text1.to_string());
                                    }
                                }
                            }
                            return Ok(result);
                        }
                    }
                    for cap in RE_MODEL.captures_iter(&content) {
                        let text1 = cap.get(1).map_or("", |m| m.as_str());
                        if text1 == key {
                            result.insert(key, dir.path().to_str().unwrap().to_string());
                            if let Some(caps) = RE_INCLUDE.captures(&content) {
                                for cap in caps.iter().skip(1) {
                                    let text1 = cap.map_or("", |m| m.as_str());
                                    if !text1.contains('/') {
                                        //when there is no slash i could be
                                        //a relative path.
                                        let mut parent = dir
                                            .path()
                                            .parent()
                                            .unwrap()
                                            .to_str()
                                            .unwrap()
                                            .to_string();
                                        parent += "/";
                                        parent += text1;
                                        result.insert(text1.to_string(), parent.to_string());
                                    } else {
                                        result.insert(text1.to_string(), text1.to_string());
                                    }
                                }
                            }
                            return Ok(result);
                        }
                    }
                }
            }
        }
        Err(Error::SpiceModelNotFound(key))
    }

    fn includes(&self) -> Vec<String> {
        let mut includes: HashMap<String, String> = HashMap::new();
        for item in &self.items {
            if let CircuitItem::X(_, _, value) = item {
                if !includes.contains_key(value) && !self.subcircuits.contains_key(value) {
                    let incs = self.get_includes(value.to_string()).unwrap();
                    for (key, value) in incs {
                        includes.entry(key).or_insert(value);
                    }
                }
            } else if let CircuitItem::Q(_, _, _, _, value) = item {
                if !includes.contains_key(value) && !self.subcircuits.contains_key(value) {
                    let incs = self.get_includes(value.to_string()).unwrap();
                    for (key, value) in incs {
                        includes.entry(key).or_insert(value);
                    }
                }
            }
        }
        let mut result = Vec::new();
        for (_, v) in includes {
            result.push(format!(".include {}\n", v).to_string());
        }
        result
    }

    fn to_str(&self, close: bool) -> Result<Vec<String>, Error> {
        let mut res = Vec::new();
        res.append(&mut self.includes());
        for (key, value) in &self.subcircuits {
            let nodes = value.0.join(" ");
            res.push(format!(".subckt {} {}", key, nodes));
            res.append(&mut value.1.to_str(false).unwrap());
            res.push(".ends".to_string());
        }
        for item in &self.items {
            match item {
                CircuitItem::R(reference, n0, n1, value) => {
                    if reference.starts_with('R') {
                        res.push(format!("{} {} {} {}", reference, n0, n1, value));
                    } else {
                        res.push(format!("R{} {} {} {}", reference, n0, n1, value));
                    }
                }
                CircuitItem::C(reference, n0, n1, value) => {
                    if reference.starts_with('C') {
                        res.push(format!("{} {} {} {}", reference, n0, n1, value));
                    } else {
                        res.push(format!("C{} {} {} {}", reference, n0, n1, value));
                    }
                }
                CircuitItem::D(reference, n0, n1, value) => {
                    if reference.starts_with('D') {
                        res.push(format!("{} {} {} {}", reference, n0, n1, value));
                    } else {
                        res.push(format!("D{} {} {} {}", reference, n0, n1, value));
                    }
                }
                CircuitItem::Q(reference, n0, n1, n2, value) => {
                    res.push(format!("Q{} {} {} {} {}", reference, n0, n1, n2, value));
                }
                CircuitItem::X(reference, n, value) => {
                    let mut nodes: String = String::new();
                    for _n in n {
                        nodes += _n;
                        nodes += " ";
                    }
                    res.push(format!("X{} {}{}", reference, nodes, value));
                }
                CircuitItem::V(reference, n0, n1, value) => {
                    res.push(format!("V{} {} {} {}", reference, n0, n1, value));
                }
            }
        }
        //TODO add options
        if close {
            res.push(String::from(".end"));
        }
        Ok(res)
    }
}

pub struct Simulation {
    pub circuit: Circuit,
    pub buffer: Option<Vec<String>>,
}

/// simulate the circuit with ngspice
/// TODO circuit models are imported twice
/// TODO create simulatio file
impl Simulation {
    /* fn subcircuit(&mut self, circuit: SubCircuit) -> None:
    """
    Add a subcircuit.
    :param circuit: Circuit to add.
    :type circuit: Circuit
    :return: None
    :rtype: None
    """
    self.subcircuits[circuit.name] = circuit */

    /* pub fn add_subcircuit(&mut self, name: &str, circuit: Circuit) {
        self.subcircuit.insert(name.to_string(), circuit);
    } */

    pub fn new(circuit: Circuit) -> Self {
        Self {
            circuit,
            buffer: None,
        }
    }

    pub fn tran(&mut self, step: &str, stop: &str, start: &str) -> HashMap<String, Vec<f64>> {
        let mut c = Cb::new();
        let ngspice = NgSpice::new(&mut c).unwrap();
        let circ = self.circuit.to_str(true).unwrap();
        ngspice.circuit(circ).unwrap();
        ngspice
            .command(format!("tran {} {} {}", step, stop, start).as_str())
            .unwrap(); //TODO
        let plot = ngspice.current_plot().unwrap();
        let res = ngspice.all_vecs(plot.as_str()).unwrap();
        let mut map: HashMap<String, Vec<f64>> = HashMap::new();
        for name in res {
            let re = ngspice.vector_info(name.as_str());
            if let Ok(r) = re {
                let name = r.name;
                let data1 = match r.data {
                    ComplexSlice::Real(list) => list.iter().map(|i| *i).collect(),
                    ComplexSlice::Complex(list) => {
                        list.iter().map(|f| {
                            if !f.cx_real.is_nan() {
                                f.cx_real
                            } else if !f.cx_imag.is_nan() {
                                f.cx_imag
                            } else {
                                todo!("can not get value from complex: {:?}", f);
                            }}).collect()
                    }
                };
                map.insert(name, data1);
            } else {
                panic!("Can not run tran with schema.");
            }
        }
        println!("tran return: {}, {}, {}", c.status, c.unload, c.quit);
        self.buffer = Some(c.strs.clone());
        map
    }
    pub fn ac(&mut self, start_frequency: &str, stop_frequency: &str, number_of_points: u32,  variation: &str) -> HashMap<String, Vec<f64>> {
        let mut c = Cb::new();
        let ngspice = NgSpice::new(&mut c).unwrap();
        let circ = self.circuit.to_str(true).unwrap();
        ngspice.circuit(circ).unwrap();
        ngspice
            //DEC ND FSTART FSTOP
            .command(format!("ac {} {} {} {}", variation, number_of_points, start_frequency, stop_frequency).as_str())
            .unwrap(); //TODO
        let plot = ngspice.current_plot().unwrap();
        let res = ngspice.all_vecs(plot.as_str()).unwrap();
        let mut map: HashMap<String, Vec<f64>> = HashMap::new();
        for name in res {
            let re = ngspice.vector_info(name.as_str());
            if let Ok(r) = re {
                let name = r.name;
                let data1 = match r.data {
                    ComplexSlice::Real(list) => list.iter().map(|i| *i).collect(),
                    ComplexSlice::Complex(list) => {
                        list.iter().map(|f| {
                            if !f.cx_real.is_nan() {
                                f.cx_real
                            } else if !f.cx_imag.is_nan() {
                                f.cx_imag
                            } else {
                                todo!("can not get value from complex: {:?}", f);
                            }}).collect()
                    }
                };
                map.insert(name, data1);
            } else {
                panic!("Can not run ac with schema.");
            }
        }
        println!("ac return: {}, {}, {}", c.status, c.unload, c.quit);
        self.buffer = Some(c.strs.clone());
        map
    }
}

#[cfg(test)]
mod tests {
    use crate::Circuit;

    #[test]
    fn load_model() {
        let circuit = Circuit::new(String::from("test"), vec![String::from("files/spice/")]);
        let include = circuit.get_includes(String::from("TL072")).unwrap();
        assert_eq!("files/spice/TL072.lib", include.get("TL072").unwrap());
        let include = circuit.get_includes(String::from("BC547B")).unwrap();
        assert_eq!("files/spice/BC547.mod", include.get("BC547B").unwrap());
        let include = circuit.get_includes(String::from("BC556B")).unwrap();
        assert_eq!("files/spice/bc5x7.lib", include.get("BC556B").unwrap());
    }
}
